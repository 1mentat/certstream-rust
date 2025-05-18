# certstream-rust

Small funemployment Rust project to build a CLI program to read the websocket stream from <https://certstream.calidog.io> and log all certificate domains to a Delta Lake table.

The location of the table is specified with the `TABLE_URI` environment variable and will be created automatically if it does not exist.

## Building

```
git clone git@github.com:hrbrmstr/certstream-rust
cargo build --release 
```

## Installing

The following will put:

- `certstream`

into `~/.cargo/bin` unless you've modified the behaviour of `cargo install`.

```
$ cargo install --git https://github.com/hrbrmstr/certstream-rust
```

## Read from CertStream websocket

The program expects the `TABLE_URI` environment variable to contain the
location of the Delta Lake table.  The table will be created if it does not
already exist.

```
USAGE:
    certstream [OPTIONS]

OPTIONS:
    -h, --help                   Print help information
    -p, --patience <PATIENCE>    [default: 5]
    -s, --server <SERVER>        [default: wss://certstream.calidog.io/]
    -V, --version                Print version information
```

Example:
```
$ TABLE_URI=./certstream.delta certstream   # press Ctrl+C to stop
```

