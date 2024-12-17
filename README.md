
# Crust

Crust is a Minecraft Layer 7 Reverse Proxy that aims for pure performance and rich features.

The software is written in Rust only and in an early development stage. We are currently supporting all minecraft versions starting at 1.20.2. We aim to improve the protocol support to 1.8 and up



## Download and Installation

Currently Linux aarch64 and x86_64 are supported.
Download the binary file that matches your OS here: https://ci.outfluencer.dev/job/Crust/
make the file executeable

```bash
  chmod 777 crust-linux-x86_64
```

Run Crust
```bash
  ./crust-linux-x86_64
```

You can also run it inside a screen or container
## Configuration
After the server is started for the first time a config.json file will be created in the same folder as the executable.

Right now you need to restart to apply config changes.
## Security
You should firewall your ports or bind you backend servers locally, otherwise someone could join your backend servers.
## Planned features

Inject into Commands packet to make our commands tabable.

Add a plugin system with API and events
