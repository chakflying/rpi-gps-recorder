extern crate adafruit_gps;
extern crate gpx;

use std::fs;
use std::process;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::RwLock;
use std::thread;
use std::time::{Duration, Instant};

use geo::prelude::*;
use geo::Point;
use gpx::{Fix, Track, TrackSegment, Waypoint};

use chrono::prelude::*;

use adafruit_gps::gga;
use adafruit_gps::NmeaOutput;
use adafruit_gps::{Gps, GpsSentence};

use rusqlite::{Connection, Result};

use ctrlc;

use tracing::{debug, error, info, warn};
use tracing_subscriber;

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
    tracing_subscriber::fmt::init();

    // Since CtrlC is handled in another thread, we need to signal the main thread to exit
    let (tx, rx) = mpsc::channel::<bool>();

    let baud_rate = "115200";
    let port = "/dev/serial0";

    adafruit_gps::set_baud_rate(baud_rate, port);

    thread::sleep(Duration::from_millis(100));

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
    info!("Set update rate to 500ms: {:?}", rate_result);

    let db = open_db().expect("Error connecting to database");

    // Store a segment in main thread.
    let segment = Arc::new(RwLock::new(TrackSegment::new()));

    // Another reference used to write to gpx file in CtrlC handler
    let out_segment = segment.clone();

    // Track last write to the segment history
    let mut last_update = Instant::now();
    let mut last_speed = 0.0;
    let mut last_speed_update = Instant::now();
    let (mut pdop, mut vdop, mut hdop) = (0.0, 0.0, 0.0);
    let mut last_dop_update = Instant::now();

    let mut saving = false;

    ctrlc::set_handler(move || {
        // Prevent duplicate save
        if saving {
            return;
        } else {
            saving = true;
        }

        let filename = format!("record-{}.gpx", Local::now().format("%FT%H%M"));
        let mut gpx_file = gpx::Gpx {
            version: gpx::GpxVersion::Gpx11,
            tracks: vec![Track::new()],
            ..gpx::Gpx::default()
        };

        let num_points = out_segment.read().unwrap().points.len();

        if num_points > 0 {
            info!("Saving segment with {} points.", num_points);
            gpx_file.tracks[0]
                .segments
                .push((*out_segment.write().unwrap()).clone());
            let mut buf = Vec::new();
            gpx::write(&gpx_file, &mut buf).unwrap();
            fs::write(&filename, buf).expect("Error Writing to file.");
        }

        tx.send(true).expect("Failed to send exit to main thread");
    })
    .expect("Error setting Ctrl-C handler");

    // In a loop, constantly update the gps. The update trait will give you all the data you
    // want from the gps module.
    loop {
        // Main thread is signal to exit
        match rx.try_recv() {
            Ok(_) => {
                db.close().expect("Failed to close database connection");
                process::exit(0);
            }
            Err(_) => {}
        }

        let values = gps.update();

        // Depending on what values you are interested in you can adjust what sentences you
        // wish to get and ignore all other sentences.
        match values {
            GpsSentence::InvalidSentence => warn!("Invalid sentence, wrong baud rate?"),
            GpsSentence::InvalidBytes => warn!("Invalid bytes given, try again"),
            GpsSentence::NoConnection => warn!("No connection with gps"),

            GpsSentence::GGA(sentence) => {
                debug!(
                    "UTC: {}, Fix: {:?}\nLat: {}, Long: {}, Sats: {}, MSL Alt: {}",
                    sentence.utc,
                    (sentence.sat_fix != gga::SatFix::NoFix),
                    sentence.lat.unwrap_or(0.0),
                    sentence.long.unwrap_or(0.0),
                    sentence.satellites_used,
                    sentence.msl_alt.unwrap_or(0.0)
                );

                // Add point to GPX if there is a fix
                if sentence.sat_fix != gga::SatFix::NoFix {
                    // Create Waypoint and write data
                    let mut waypoint = Waypoint::new(Point::new(
                        sentence.long.unwrap_or(0.0).into(),
                        sentence.lat.unwrap_or(0.0).into(),
                    ));

                    waypoint.time = Some(Utc::now());

                    waypoint.elevation = Some((sentence.msl_alt.unwrap_or(0.0)).into());

                    waypoint.fix = if sentence.satellites_used == 3 {
                        Some(Fix::TwoDimensional)
                    } else {
                        Some(Fix::ThreeDimensional)
                    };

                    waypoint.sat = Some(sentence.satellites_used as u64);

                    waypoint.source = Some("MTK3339".into());

                    if last_speed_update.elapsed().as_secs() < 3 {
                        waypoint.speed = Some(last_speed / 3.6);
                    }

                    if last_dop_update.elapsed().as_secs() < 3 {
                        waypoint.vdop = Some(vdop);
                        waypoint.hdop = Some(hdop);
                        waypoint.pdop = Some(pdop);
                    }

                    // Write to database
                    let mut stmt = db
                        .prepare_cached("INSERT INTO location_history (waypoint) VALUES (?)")
                        .expect("Failed to prepare SQL statement");

                    let save_result = stmt.execute([
                        serde_json::to_string(&waypoint).expect("Failed to serialize location")
                    ]);
                    match save_result {
                        Err(e) => {
                            error!("Error when saving to database: {:?}", e)
                        }
                        _ => {}
                    }

                    if let Some(last_point) = segment.write().unwrap().points.last_mut() {
                        if last_point.point().geodesic_distance(&waypoint.point()) < 3.0
                            && last_update.elapsed().as_secs() < 5
                        {
                            debug!("No significant movement, skipping write.");
                            continue;
                        }
                    }
                    segment.write().unwrap().points.push(waypoint);
                    last_update = Instant::now();
                }
            }

            GpsSentence::GSA(sentence) => {
                info!(
                    "PDOP: {}, VDOP:{}, HDOP:{}",
                    sentence.pdop.unwrap_or(0.0),
                    sentence.vdop.unwrap_or(0.0),
                    sentence.hdop.unwrap_or(0.0)
                );

                last_dop_update = Instant::now();

                pdop = sentence.pdop.unwrap_or(0.0).into();
                vdop = sentence.vdop.unwrap_or(0.0).into();
                hdop = sentence.hdop.unwrap_or(0.0).into();

                if let Some(last_point) = segment.write().unwrap().points.last_mut() {
                    last_point.hdop = Some(sentence.hdop.unwrap_or(0.0).into());
                    last_point.vdop = Some(sentence.vdop.unwrap_or(0.0).into());
                    last_point.pdop = Some(sentence.pdop.unwrap_or(0.0).into());
                }
            }

            GpsSentence::GSV(sentence) => {
                debug!(
                    "Signal Strength (dB): {:?}",
                    sentence
                        .iter()
                        .map(|x| x.snr.unwrap_or(0.0))
                        .collect::<Vec<_>>()
                );
            }

            GpsSentence::VTG(sentence) => {
                info!("Speed kph: {}", sentence.speed_kph.unwrap_or(0.0));
                last_speed = sentence.speed_kph.unwrap_or(0.0).into();
                last_speed_update = Instant::now();
            }

            _ => (),
        }
    }
}
