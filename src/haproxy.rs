use std::net::{SocketAddrV4, SocketAddrV6};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::util::{IOError, IOErrorKind, IOResult};

const VERSION_1_MAGIC: &[u8] = b"PROXY";
const VERSION_2_MAGIC: &[u8] = b"\x0D\x0A\x0D\x0A\x00\x0D\x0A\x51\x55\x49\x54\x0A";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HAProxyMessage {
    V1(HAProxyMessageV1),
    V2(HAProxyMessageV2),
}

impl HAProxyMessage {
    pub async fn decode_async<R: AsyncRead + Unpin + ?Sized>(source: &mut R) -> IOResult<Self> {
        let mut buf = [0u8; 5];
        source.read_exact(&mut buf).await?;
        if buf.as_slice() == VERSION_1_MAGIC {
            if source.read_u8().await? != b' ' {
                return Err(IOError::new(
                    IOErrorKind::InvalidData,
                    "Invalid HAProxyV1 protocol: missing space after magic",
                ));
            }
            let mut buf = [0u8; 108 - 6];
            for i in 0..buf.len() {
                buf[i] = source.read_u8().await?;
                if i > 0 && buf[i - 1] == b'\r' && buf[i] == b'\n' {
                    break;
                }
            }
            let mut index = None;
            for i in 1..buf.len() {
                if buf[i - 1] == b'\r' && buf[i] == b'\n' {
                    index = Some(i - 1);
                    break;
                }
            }
            if index.is_none() {
                return Err(IOError::new(
                    IOErrorKind::InvalidData,
                    "Invalid HAProxyV1 protocol: missing CRLF",
                ));
            }
            let text = String::from_utf8_lossy(&buf[..index.unwrap()]).to_string();
            let split = text.split(' ').collect::<Vec<_>>();
            if split.len() >= 1 && split[0] == "UNKNOWN" {
                return Ok(Self::V1(HAProxyMessageV1 {
                    protocol_family: HAProxyProtocolFamily::Unknown,
                }));
            }
            if split.len() != 5 {
                return Err(IOError::new(
                    IOErrorKind::InvalidData,
                    "Invalid HAProxyV1 protocol: invalid number of fields",
                ));
            }
            let proto_type = split[0];
            let src_ip = split[1];
            let dst_ip = split[2];
            let src_port = split[3];
            let dst_port = split[4];
            return Ok(Self::V1(HAProxyMessageV1 {
                protocol_family: match proto_type {
                    "TCP4" => HAProxyProtocolFamily::TCP4 {
                        src: SocketAddrV4::new(
                            src_ip.parse().map_err(|_| {
                                IOError::new(IOErrorKind::InvalidData, "Invalid source ip")
                            })?,
                            src_port.parse().map_err(|_| {
                                IOError::new(IOErrorKind::InvalidData, "Invalid source port")
                            })?,
                        ),
                        dst: SocketAddrV4::new(
                            src_ip.parse().map_err(|_| {
                                IOError::new(IOErrorKind::InvalidData, "Invalid source ip")
                            })?,
                            src_port.parse().map_err(|_| {
                                IOError::new(IOErrorKind::InvalidData, "Invalid source port")
                            })?,
                        ),
                    },
                    "TCP6" => HAProxyProtocolFamily::TCP6 {
                        src: SocketAddrV6::new(
                            dst_ip.parse().map_err(|_| {
                                IOError::new(IOErrorKind::InvalidData, "Invalid source ip")
                            })?,
                            dst_port.parse().map_err(|_| {
                                IOError::new(IOErrorKind::InvalidData, "Invalid source port")
                            })?,
                            0,
                            0,
                        ),
                        dst: SocketAddrV6::new(
                            dst_ip.parse().map_err(|_| {
                                IOError::new(IOErrorKind::InvalidData, "Invalid source ip")
                            })?,
                            dst_port.parse().map_err(|_| {
                                IOError::new(IOErrorKind::InvalidData, "Invalid source port")
                            })?,
                            0,
                            0,
                        ),
                    },
                    _ => {
                        return Err(IOError::new(
                            IOErrorKind::InvalidData,
                            "Invalid HAProxyV1 protocol: invalid protocol type",
                        ))
                    }
                },
            }));
        }
        let mut buf2 = [0u8; 12];
        buf2[..5].copy_from_slice(&buf);
        source.read_exact(&mut buf2[5..]).await?;
        if buf2.as_slice() == VERSION_2_MAGIC {
            let ver = source.read_u8().await?;
            let version = ver >> 4;
            if version != 2 {
                return Err(IOError::new(
                    IOErrorKind::InvalidData,
                    "Invalid HAProxyV2 version",
                ));
            }
            let command = match ver & 0xF {
                0 => HAProxyCommand::Local,
                1 => HAProxyCommand::Proxy,
                _ => {
                    return Err(IOError::new(
                        IOErrorKind::InvalidData,
                        "Invalid HAProxyV2 command",
                    ))
                }
            };
            let fam = source.read_u8().await?;
            let address_family = match fam >> 4 {
                0 => HAPRoxyAddressFamily::Unspec,
                1 => HAPRoxyAddressFamily::Inet,
                2 => HAPRoxyAddressFamily::Inet6,
                3 => HAPRoxyAddressFamily::Unix,
                _ => {
                    return Err(IOError::new(
                        IOErrorKind::InvalidData,
                        "Invalid HAProxyV2 address family",
                    ))
                }
            };
            let transport_protocol = match fam & 0xF {
                0 => HAProxyTransportProtocol::Unspec,
                1 => HAProxyTransportProtocol::Stream,
                2 => HAProxyTransportProtocol::Dgram,
                _ => {
                    return Err(IOError::new(
                        IOErrorKind::InvalidData,
                        "Invalid HAProxyV2 transport protocol",
                    ))
                }
            };
            let mut len = source.read_u16().await?;

            let addresses = match address_family {
                HAPRoxyAddressFamily::Inet => match transport_protocol {
                    HAProxyTransportProtocol::Stream | HAProxyTransportProtocol::Dgram => {
                        if len < 12 {
                            return Err(IOError::new(
                                IOErrorKind::InvalidData,
                                "Invalid HAProxyV2 address length for TCP/UDP over IPv4",
                            ));
                        }
                        len -= 12;
                        let mut src = [0u8; 4];
                        source.read_exact(&mut src).await?;
                        let mut dst = [0u8; 4];
                        source.read_exact(&mut dst).await?;
                        let src = SocketAddrV4::new(src.into(), source.read_u16().await?);
                        let dst = SocketAddrV4::new(dst.into(), source.read_u16().await?);
                        HAProxyAdresses::Inet { src, dst }
                    }
                    HAProxyTransportProtocol::Unspec => {
                        return Err(IOError::new(
                            IOErrorKind::InvalidData,
                            "Unexpected HAProxyV2 transport protocol: UNSPEC",
                        ))
                    }
                },
                HAPRoxyAddressFamily::Inet6 => match transport_protocol {
                    HAProxyTransportProtocol::Stream | HAProxyTransportProtocol::Dgram => {
                        if len < 36 {
                            return Err(IOError::new(
                                IOErrorKind::InvalidData,
                                "Invalid HAProxyV2 address length for TCP/UDP over IPv4",
                            ));
                        }
                        len -= 36;
                        let mut src = [0u8; 16];
                        source.read_exact(&mut src).await?;
                        let mut dst = [0u8; 16];
                        source.read_exact(&mut dst).await?;
                        let src = SocketAddrV6::new(src.into(), source.read_u16().await?, 0, 0);
                        let dst = SocketAddrV6::new(dst.into(), source.read_u16().await?, 0, 0);
                        HAProxyAdresses::Inet6 { src, dst }
                    }
                    HAProxyTransportProtocol::Unspec => {
                        return Err(IOError::new(
                            IOErrorKind::InvalidData,
                            "Unexpected HAProxyV2 transport protocol: UNSPEC",
                        ))
                    }
                },
                HAPRoxyAddressFamily::Unix => {
                    if len < 216 {
                        return Err(IOError::new(
                            IOErrorKind::InvalidData,
                            "Invalid HAProxyV2 address length for UNIX",
                        ));
                    }
                    len -= 216;
                    let mut src = [0u8; 108];
                    source.read_exact(&mut src).await?;
                    let mut dst = [0u8; 108];
                    source.read_exact(&mut dst).await?;
                    let src = String::from_utf8_lossy(
                        &src.into_iter().take_while(|&c| c != 0).collect::<Vec<_>>(),
                    )
                    .to_string();
                    let dst = String::from_utf8_lossy(
                        &dst.into_iter().take_while(|&c| c != 0).collect::<Vec<_>>(),
                    )
                    .to_string();
                    HAProxyAdresses::Unix { src, dst }
                }
                HAPRoxyAddressFamily::Unspec => HAProxyAdresses::Unspec,
            };
            source.take(len as u64).read_to_end(&mut Vec::new()).await?;
            return Ok(HAProxyMessage::V2(HAProxyMessageV2 {
                command,
                address_family,
                transport_protocol,
                addresses,
            }));
        }
        Err(IOError::new(
            IOErrorKind::InvalidData,
            "Failed to identify HAProxy protocol version",
        ))
    }

    pub async fn encode_async<W: AsyncWrite + Unpin + ?Sized>(&self, dest: &mut W) -> IOResult<()> {
        match self {
            Self::V1(_) => {
                todo!()
            }
            Self::V2(HAProxyMessageV2 {
                command,
                address_family,
                transport_protocol,
                addresses,
            }) => {
                dest.write_all(VERSION_2_MAGIC).await?;
                dest.write_u8((2 << 4) | *command as u8).await?;
                dest.write_u8((*address_family as u8) << 4 | (*transport_protocol as u8))
                    .await?;
                match addresses {
                    HAProxyAdresses::Inet { src, dst } => {
                        dest.write_u16(12).await?;
                        dest.write_all(&src.ip().octets()).await?;
                        dest.write_all(&dst.ip().octets()).await?;
                        dest.write_u16(src.port()).await?;
                        dest.write_u16(dst.port()).await?;
                    }
                    HAProxyAdresses::Inet6 { src, dst } => {
                        dest.write_u16(36).await?;
                        dest.write_all(&src.ip().octets()).await?;
                        dest.write_all(&dst.ip().octets()).await?;
                        dest.write_u16(src.port()).await?;
                        dest.write_u16(dst.port()).await?;
                    }
                    HAProxyAdresses::Unix { src, dst } => {
                        dest.write_u16(216).await?;
                        Self::write_padded_string(dest, &src, 108).await?;
                        Self::write_padded_string(dest, &dst, 108).await?;
                    }
                    HAProxyAdresses::Unspec => {
                        dest.write_u16(0).await?;
                    }
                }
            }
        }
        Ok(())
    }

    async fn write_padded_string<W: AsyncWrite + Unpin + ?Sized>(
        dest: &mut W,
        s: &str,
        len: usize,
    ) -> IOResult<()> {
        let str_len = s.len().min(len);
        for c in s.chars().take(str_len) {
            dest.write_u8(c as u8).await?;
        }
        for _ in 0..(len - str_len) {
            dest.write_u8(0).await?;
        }
        Ok(())
    }
}

impl From<HAProxyMessageV2> for HAProxyMessage {
    fn from(v2: HAProxyMessageV2) -> Self {
        HAProxyMessage::V2(v2)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HAProxyMessageV1 {
    pub protocol_family: HAProxyProtocolFamily,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HAProxyProtocolFamily {
    TCP4 {
        src: SocketAddrV4,
        dst: SocketAddrV4,
    },
    TCP6 {
        src: SocketAddrV6,
        dst: SocketAddrV6,
    },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HAProxyMessageV2 {
    pub command: HAProxyCommand,
    pub address_family: HAPRoxyAddressFamily,
    pub transport_protocol: HAProxyTransportProtocol,
    pub addresses: HAProxyAdresses,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HAProxyCommand {
    Local,
    Proxy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HAPRoxyAddressFamily {
    Unspec,
    Inet,
    Inet6,
    Unix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HAProxyTransportProtocol {
    Unspec,
    Stream,
    Dgram,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HAProxyAdresses {
    Inet {
        src: SocketAddrV4,
        dst: SocketAddrV4,
    },
    Inet6 {
        src: SocketAddrV6,
        dst: SocketAddrV6,
    },
    Unix {
        src: String,
        dst: String,
    },
    Unspec,
}
