# jojo-client

The embedded component in this repository is a crucial part of [jojo](https://github.com/gggiulio77/jojo). Its function is to transmit user inputs to the [jojo-server](https://github.com/gggiulio77/jojo=server) through a WiFi network.

## Getting Started

The project is structured as a [cargo workspace](https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html), allowing for the reuse of common functions across different packages. It supports the creation of N devices with varying hardware requirements such as ADC channels, GPIO, I2C, etc.

Currently, it exclusively supports the [ESP32-S3 SoC](https://www.espressif.com/en/products/socs/esp32-s3). All development has been done using the [ESP32-S3-C1-N8R8](https://docs.espressif.com/projects/esp-idf/en/latest/esp32s3/hw-reference/esp32s3/user-guide-devkitc-1.html) devkit purchased from [Mouser](https://ar.mouser.com/ProductDetail/356-EP32S3DVKTC1N8R8).

This application utilizes the [Espressif ecosystem for Rust](https://github.com/esp-rs), where ESP-IDF (a fork of FreeRTOS) serves as the operating system, and all functionalities are implemented as "tasks." Communication between these tasks is facilitated through channels, condvars, mutexes, etc.

The client operates in two modes:

OTP Mode: Initially acts as an access point, allowing users to connect to it and make requests to its HTTP server. This mode facilitates scanning WiFi networks and storing credentials. It only switches to this mode if the device doesn't find any network credentials in flash.

WebSocket Client Mode: With credentials stored in flash, the client connects to the WiFi network and appears as a device connected to it. It first searches for the [jojo-server](https://github.com/gggiulio77/jojo-server) using [jojo-discovery](https://github.com/gggiulio77/jojo-discovery). Upon discovery, it attempts to establish a WebSocket connection with the server. Once connected, it starts transmitting all user inputs to the server. The WebSocket protocol is chosen for its ability to achieve low latency between user inputs, providing a smooth user experience, particularly when controlling the mouse or virtual joystick of the host computer.

### Quick Links

- [Getting Started](#getting-started)
- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Usage](#usage)
- [Roadmap](#roadmap)
- [License](#license)

### Prerequisites

If you're new to embedded development in Rust, you may find the following resources helpful:

- [The Rust on ESP Book](https://esp-rs.github.io/book/)
- [The Embedded Rust Book](https://docs.rust-embedded.org/book/)
- [Curated list of resources](https://github.com/rust-embedded/awesome-embedded-rust)

Before getting started, make sure you have [Rust](https://www.rust-lang.org/tools/install) installed on your system.

To set up your environment, you'll need to install [espup](https://github.com/esp-rs/espup). Within the repository, you'll find various installation approaches. For instance, you can use the following command: `cargo binstall espup`. The `espup install` command will handle the setup of all target/toolchain-related components. You can customize the target, compilation flags, environment variables for configuring ESP-IDF, etc., in the `.cargo/config.toml` project file.

Finally, to flash the micro-controller, you'll need to install [espflash](https://github.com/esp-rs/espflash). Similar to [espup](https://github.com/esp-rs/espup), you have multiple installation options. For instance, you can use: `cargo binstall espflash`.

### Installation

If you're using Windows, it's advisable to clone the repository into a top-level directory like `C://` or utilize a virtual directory to mitigate potential [LONG_PATH errors](https://github.com/esp-rs/esp-idf-sys/issues/252) that may arise during the compilation of ESP ecosystem dependencies.

Here's the command to clone the repository:

`git clone https://github.com/gggiulio77/jojo-client.git`

## Usage

To compile and flash the binary, execute `cargo run -r -p mouse` or `cargo run -r -p joystick`. This command prompts for the COM port to use and initiates the binary upload process. Upon completion, it logs all console prints. The initial run may take a few minutes as it compiles all dependencies and downloads and compiles the ESP-IDF.

## Roadmap

- [ ] Enhance documentation
- [ ] Improve error handling
- [ ] Refactor initial implementation sections
- [ ] Extend support to additional devices in the ESP32 family
- [ ] Restructure the utilization of peripherals
- [ ] Implement authentication/encryption for sensitive data
- [ ] Revise the device's configuration structure

## License
