# Norppalivebot
# Norppalive Service

Norppalive Service is an application designed to detect Saimaa ringed seals from the Finnish WWF livestream [Norppalive](https://wwf.fi/luontolive/norppalive/). This README provides an overview of the project, setup instructions, and usage guidelines.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Configuration](#configuration)
- [Usage](#usage)
- [Running Tests](#running-tests)
- [License](#license)
- [Contact](#contact)

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install)
- [Python](https://www.python.org/downloads/) (for yt-dlp)
- [yt-dlp](https://github.com/yt-dlp/yt-dlp) (to get the stream URL from YouTube livestream)
- [ffmpeg](https://ffmpeg.org)

## Installation

1. Clone the repository:
    ```sh
    git clone https://github.com/dilaz/norppalive_service.git
    ```
2. Navigate to the project directory:
    ```sh
    cd norppalive_service
    ```
3. Install dependencies:
    ```sh
    cargo install
    ```
4. Make sure yt-dlp is installed
    ```sh
    pip install -U yt-dlp
    ```
5. Copy config template to `config.toml` and fill in the missing info
    ```sh
    cp config.toml.example config.toml
    ```

## Configuration

Fill in the `config.toml` file with the necessary information. Refer to the comments in the `config.toml.example` for guidance.

## Usage

This is part of the implementation that captures frames from Norppalive livestream and sends them to the [detector](https://github.com/Dilaz/Norppalivebot_detector)

To start detector, run:
```sh
cargo run
```

## Running Tests

To run tests, use the following command:
```sh
cargo test
```

Currently there are no tests

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

## Contact

For any questions or feedback, please open an issue on the GitHub repository or contact the maintainers.
