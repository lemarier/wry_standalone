# wry standalone

This is an attempt to bring WRY/Tauri to a standalone compiled binary allowing you to run a project and compile (embed the assets) into the binary without touching rust.

## Run example

### Development mode
```
cargo build --release
./target/release/wry run ./examples/project1/src/index.html
```

### Compiled mode

This will embedded all the assets into the binary and generate a new binary.

```
./target/release/wry compile ./examples/project1/src/index.html
```

This will generate a new binary `compiled-bin-test` who is a self contained wry application.