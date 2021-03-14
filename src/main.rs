extern crate adafruit_gps;
extern crate gpx;

use std::fs;
use std::sync::mpsc;
use std::thread;

use geo_types::Point;
use gpx::{Fix, Track, TrackSegment, Waypoint};

use chrono::prelude::*;

use adafruit_gps::gga;
use adafruit_gps::NmeaOutput;
use adafruit_gps::{Gps, GpsSentence};

fn main() {
    let (tx, rx) = mpsc::channel::<TrackSegment>();
    let baud_rate = "115200";
    let port = "/dev/serial0";

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

    // Store a segment in main thread.
    let mut segment = TrackSegment::new();

    // Use another thread for IO.
    thread::spawn(move || {
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
            fs::write("test.gpx", buf).expect("Error Writing to file.");
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
                println!(
                    "UTC: {}, Fix: {:?}\nLat: {}, Long: {}, Sats: {}, MSL Alt: {}",
                    sentence.utc,
                    (sentence.sat_fix != gga::SatFix::NoFix),
                    sentence.lat.unwrap_or(0.0),
                    sentence.long.unwrap_or(0.0),
                    sentence.satellites_used,
                    sentence.msl_alt.unwrap_or(0.0)
                );
                if segment.points.len() > 10 {
                    tx.send(segment).unwrap();
                    segment = TrackSegment::new();
                }

                // Add point to GPX if there is a fix
                if sentence.sat_fix != gga::SatFix::NoFix {
                    let mut point = Waypoint::new(Point::new(
                        sentence.long.unwrap_or(0.0).into(),
                        sentence.lat.unwrap_or(0.0).into(),
                    ));
                    point.time = Some(Utc::now());
                    point.elevation = Some((sentence.msl_alt.unwrap_or(0.0)).into());
                    point.fix = Some(Fix::ThreeDimensional);
                    point.sat = Some(sentence.satellites_used as u64);
                    point.source = Some("MTK3339".into());
                    segment.points.push(point);
                }
            }
            GpsSentence::GSA(sentence) => {
                println!(
                    "PDOP: {}, VDOP:{}, HDOP:{}",
                    sentence.pdop.unwrap_or(0.0),
                    sentence.vdop.unwrap_or(0.0),
                    sentence.hdop.unwrap_or(0.0)
                );
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
            }
            _ => (),
        }
    }
}
