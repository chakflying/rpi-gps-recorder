# rpi-gps-recorder

This is a simple command-line program that takes gps data from the Adafruit Ultimate GPS and writes to a .gpx file.

It assumes the MTK3339 is pre-configured with baudrate 115200, and is on the pi's `/dev/serial0`.

Update rate is set to 2Hz, and can be configured in code.

## Libraries used:

- [adafruit_gps](https://github.com/MechanicalPython/adafruit_gps): which parses longitude incorrectly, and needs to be manually patched (hence the local dependency config)
- [gpx](https://github.com/georust/gpx)
- [geo-types](https://github.com/georust/geo): old version used for compatibility with gpx
