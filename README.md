# Crust 💡

Crust is a Minecraft Layer 7 Reverse Proxy that aims for pure performance and rich features.

The software is written in Rust only and in an early development stage. We are currently supporting all minecraft
versions starting at 1.20.2. We aim to improve the protocol support to 1.8 and up

## Download and Installation 💿

Currently Linux aarch64 and x86_64 are available in compiled form.

Download the binary file that matches your OS on [Jenkins](https://ci.outfluencer.dev/job/Crust/)

make the file executeable

```bash
  chmod +x crust-linux-x86_64
```

Run Crust

```bash
  ./crust-linux-x86_64
```

You can also run it inside a screen or container

## Configuration ⚙️

After the server is started for the first time a config.json file will be created in the same folder as the executable.

Right now you need to restart to apply config changes.

## Security 🔗

You should firewall the ports of you backend servers or bind you backend servers locally, otherwise someone could join
your backend servers without authentication.

## Features 📃

- [x] Joining to, forwarding and switching server
- [x] configurable packet limiter
- [x] configurable fallback system (server priority system)
- [x] simple /server command
- [x] compression and encryption support for client and server connections
- [x] online and Offline Mode support
- [x] spigot data/ip forwarding support
- [x] configurable connection throttle
- [x] logging system
- [x] de-/serializing NBT
- [x] de-/serializing Chat components
- [x] versioning in binary file
- [x] inject into Commands packet to make our commands tabable
- [x] HA-Proxy support
- [ ] add a plugin system with API and events
- [x] simple permission system
- [x] good terminal UI
- [x] command system
- [ ] limbo
- [ ] support BungeeCord plugin messaging
- [ ] redis

## Build 🔨

install rust and cargo
clone this repo

run the following command in the repos directory:
cargo build --release

## Contribute 🖋️

If you want to contribute just fork the project and create a Pull Request
Our team will take a look at your work and will decide if it will be merged or need changes real quick

## Support us ⭐️

If you're interested in this project, we would appreciate it very much if you would star the repository


