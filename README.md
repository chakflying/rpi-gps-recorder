# rpi-gps-recorder

This is a simple command-line program that takes gps data from the Adafruit Ultimate GPS and writes to a .gpx file.

It assumes the MTK3339 is pre-configured with baudrate 115200, and is on the pi's `/dev/serial0`.

Update rate is set to 2Hz, and can be configured in code.

Tested on:

- Raspberry Pi 4 2GB, running on 32-bit OS
- Adafruit Ultimate GPS Pi HAT
- rustc 1.50.0 compiling to `armv7-unknown-linux-gnueabihf`

## Libraries used:

- [adafruit_gps](https://github.com/MechanicalPython/adafruit_gps)
- [gpx](https://github.com/georust/gpx)
- [geo-types](https://github.com/georust/geo)
