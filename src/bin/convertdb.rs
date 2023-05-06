use std::fs;

use gpx::{Track, TrackSegment, Waypoint};

use time::macros::{format_description, offset};
use time::OffsetDateTime;

use tracing::{error, info};
use tracing_subscriber;

use rpi_gps_recorder::database;

fn main() {
    tracing_subscriber::fmt::init();

    // Create a segment
    let mut segment = TrackSegment::new();

    let db = database::open_db().expect("Error connecting to database");

    let mut stmt = db
        .prepare_cached("SELECT * FROM location_history")
        .expect("Failed to prepare SQL statement");
    let waypoints = stmt
        .query_map([], |row| row.get::<usize, String>(1))
        .expect("Failed to run query");

    for row_result in waypoints {
        match row_result {
            Ok(waypoint_json) => {
                let value = serde_json::from_str::<Waypoint>(&waypoint_json);
                match value {
                    Ok(waypoint) => {
                        segment.points.push(waypoint);
                    }
                    Err(err) => {
                        error!("Failed to deserialize waypoint");
                        error!("{:?}", err);
                        break;
                    }
                }
            }
            Err(_) => {}
        }
    }

    let filename = format!(
        "record-{}.gpx",
        OffsetDateTime::now_utc()
            .replace_offset(offset!(+8))
            .format(format_description!(
                "[year]-[month]-[day]-[hour][minute][second]"
            ))
            .unwrap()
    );
    let mut gpx_file = gpx::Gpx {
        version: gpx::GpxVersion::Gpx11,
        tracks: vec![Track::new()],
        ..gpx::Gpx::default()
    };

    let num_points = segment.points.len();

    if num_points > 0 {
        info!("Saving segment with {} points.", num_points);
        gpx_file.tracks[0].segments.push(segment);

        let mut buf = Vec::new();
        gpx::write(&gpx_file, &mut buf).unwrap();
        fs::write(&filename, buf).expect("Error Writing to file.");
    }
}
