# Norppalivebot
# Norppalive Service

Norppalive Service is an application designed to detect Saimaa ringed seals from the Finnish WWF livestream [Norppalive](https://wwf.fi/luontolive/norppalive/) This README provides an overview of the project, setup instructions, and usage guidelines.

## Table of Contents

- [Installation](#installation)
- [Usage](#usage)
- [License](#license)

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


## Usage

This is part of the implementation that captures frames from 

To start detector, run:
```sh
cargo run
```

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.