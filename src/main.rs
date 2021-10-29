extern crate adafruit_gps;
extern crate gpx;

use std::fs;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use geo_types::Point;
use gpx::{Fix, Track, TrackSegment, Waypoint};

use chrono::prelude::*;

use adafruit_gps::gga;
use adafruit_gps::NmeaOutput;
use adafruit_gps::{Gps, GpsSentence};

use rusqlite::{Connection, Result};

// Open and prepare Database
fn open_db() -> Result<Connection> {
    let db_path = "./data.db";
    let db = Connection::open(&db_path)?;
    db.pragma_update(None, "foreign_keys", "ON")
        .expect("Failed to set PRAGMA");
    db.pragma_update(None, "journal_mode", "WAL")
        .expect("Failed to set PRAGMA");
    db.pragma_update(None, "auto_vacuum", "FULL")
        .expect("Failed to set PRAGMA");

    db.execute(
        "CREATE TABLE IF NOT EXISTS location_history (
                  id              INTEGER PRIMARY KEY AUTOINCREMENT,
                  waypoint        TEXT NOT NULL,
                  createdAt       TEXT DEFAULT CURRENT_TIMESTAMP
                  )",
        [],
    )
    .expect("Failed when checking for history table in database");

    Ok(db)
}

fn main() {
    let (tx, rx) = mpsc::channel::<TrackSegment>();
    let baud_rate = "115200";
    let port = "/dev/serial0";

    // Store last update
    let mut last_update = Instant::now();

    let mut last_speed = 0.0;
    let mut last_speed_update = Instant::now();
    let (mut pdop, mut vdop, mut hdop) = (0.0, 0.0, 0.0);
    let mut last_dop_update = Instant::now();

    // Open the port that is connected to the GPS module.
    let mut gps = Gps::new(port, baud_rate);

    // Give settings here.
    gps.pmtk_314_api_set_nmea_output(NmeaOutput {
        gga: 1,
        gsa: 1,
        gsv: 1,
        gll: 1,
        rmc: 1,
        vtg: 1,
        pmtkchn_interval: 1,
    });
    let rate_result = gps.pmtk_220_set_nmea_updaterate("500");
    println!("Set update rate to 500ms: {:?}", rate_result);

    let db = open_db().expect("Error connecting to database");

    // Store a segment in main thread.
    let mut segment = TrackSegment::new();

    // Use another thread for IO.
    thread::spawn(move || {
        let filename = format!("record-{}.gpx", Local::now().format("%FT%H%M"));
        let mut gpx_file = gpx::Gpx {
            version: gpx::GpxVersion::Gpx11,
            tracks: vec![Track::new()],
            ..gpx::Gpx::default()
        };
        for received in rx {
            println!("Got segment with {} points.", received.points.len());
            gpx_file.tracks[0].segments.push(received);
            let mut buf = Vec::new();
            gpx::write(&gpx_file, &mut buf).unwrap();
            fs::write(&filename, buf).expect("Error Writing to file.");
        }
    });

    // In a loop, constantly update the gps. The update trait will give you all the data you
    // want from the gps module.
    loop {
        let values = gps.update();

        // Depending on what values you are interested in you can adjust what sentences you
        // wish to get and ignore all other sentences.
        match values {
            GpsSentence::InvalidSentence => println!("Invalid sentence, try again"),
            GpsSentence::InvalidBytes => println!("Invalid bytes given, try again"),
            GpsSentence::NoConnection => println!("No connection with gps"),

            GpsSentence::GGA(sentence) => {
                // println!(
                //     "UTC: {}, Fix: {:?}\nLat: {}, Long: {}, Sats: {}, MSL Alt: {}",
                //     sentence.utc,
                //     (sentence.sat_fix != gga::SatFix::NoFix),
                //     sentence.lat.unwrap_or(0.0),
                //     sentence.long.unwrap_or(0.0),
                //     sentence.satellites_used,
                //     sentence.msl_alt.unwrap_or(0.0)
                // );
                if segment.points.len() > 200
                    || (segment.points.len() > 0 && last_update.elapsed().as_secs() > 3)
                {
                    tx.send(segment).unwrap();
                    segment = TrackSegment::new();
                }

                // Add point to GPX if there is a fix
                if sentence.sat_fix != gga::SatFix::NoFix {
                    // Create Waypoint and write data
                    let mut point = Waypoint::new(Point::new(
                        sentence.long.unwrap_or(0.0).into(),
                        sentence.lat.unwrap_or(0.0).into(),
                    ));

                    point.time = Some(Utc::now());

                    point.elevation = Some((sentence.msl_alt.unwrap_or(0.0)).into());

                    point.fix = if sentence.satellites_used == 3 {
                        Some(Fix::TwoDimensional)
                    } else {
                        Some(Fix::ThreeDimensional)
                    };

                    point.sat = Some(sentence.satellites_used as u64);

                    point.source = Some("MTK3339".into());

                    if last_speed_update.elapsed().as_secs() < 3 {
                        point.speed = Some(last_speed / 3.6);
                    }

                    if last_dop_update.elapsed().as_secs() < 3 {
                        point.vdop = Some(vdop);
                        point.hdop = Some(hdop);
                        point.pdop = Some(pdop);
                    }

                    // Write to database
                    let mut stmt = db
                        .prepare_cached("INSERT INTO location_history (waypoint) VALUES (?)")
                        .expect("Failed to prepare SQL statement");

                    let save_result = stmt.execute([
                        serde_json::to_string(&point).expect("Failed to serialize location")
                    ]);
                    match save_result {
                        Err(e) => {
                            println!("Error when saving to database: {:?}", e)
                        }
                        _ => {}
                    }

                    segment.points.push(point);
                    last_update = Instant::now();
                }
            }

            GpsSentence::GSA(sentence) => {
                println!(
                    "PDOP: {}, VDOP:{}, HDOP:{}",
                    sentence.pdop.unwrap_or(0.0),
                    sentence.vdop.unwrap_or(0.0),
                    sentence.hdop.unwrap_or(0.0)
                );

                last_dop_update = Instant::now();

                pdop = sentence.pdop.unwrap_or(0.0).into();
                vdop = sentence.vdop.unwrap_or(0.0).into();
                hdop = sentence.hdop.unwrap_or(0.0).into();

                if let Some(last_point) = segment.points.last_mut() {
                    last_point.hdop = Some(sentence.hdop.unwrap_or(0.0).into());
                    last_point.vdop = Some(sentence.vdop.unwrap_or(0.0).into());
                    last_point.pdop = Some(sentence.pdop.unwrap_or(0.0).into());
                }
            }

            GpsSentence::GSV(sentence) => {
                println!(
                    "Signal Strength (dB): {:?}",
                    sentence
                        .iter()
                        .map(|x| x.snr.unwrap_or(0.0))
                        .collect::<Vec<_>>()
                );
            }

            GpsSentence::VTG(sentence) => {
                println!("Speed kph: {}", sentence.speed_kph.unwrap_or(0.0));
                last_speed = sentence.speed_kph.unwrap_or(0.0).into();
                last_speed_update = Instant::now();
            }

            _ => (),
        }
    }
}
