use serde::{Deserialize, Serialize};

/// Socket activation specification (maps to systemd `.socket` units /
/// launchd `Sockets`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SocketSpec {
    /// Listen address (e.g., `"127.0.0.1:8080"` or `"/run/myservice.sock"`).
    pub listen: String,

    /// Socket type.
    #[serde(default)]
    pub socket_type: SocketType,

    /// File descriptor name (`LISTEN_FDNAMES`).
    #[serde(default)]
    pub name: Option<String>,
}

/// Type of socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SocketType {
    /// TCP / Unix stream socket (default).
    #[default]
    Stream,
    /// UDP / Unix datagram socket.
    Datagram,
    /// Sequential packet socket.
    Sequential,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_spec_from_yaml() {
        let yaml = r"
listen: '127.0.0.1:8080'
";
        let spec: SocketSpec = serde_yaml_ng::from_str(yaml).expect("parse");
        assert_eq!(spec.listen, "127.0.0.1:8080");
        assert_eq!(spec.socket_type, SocketType::Stream);
        assert!(spec.name.is_none());
    }

    #[test]
    fn unix_socket_with_name() {
        let yaml = r"
listen: /run/myservice.sock
socket_type: stream
name: main
";
        let spec: SocketSpec = serde_yaml_ng::from_str(yaml).expect("parse");
        assert_eq!(spec.listen, "/run/myservice.sock");
        assert_eq!(spec.name.as_deref(), Some("main"));
    }

    #[test]
    fn datagram_socket() {
        let yaml = r"
listen: '0.0.0.0:5514'
socket_type: datagram
";
        let spec: SocketSpec = serde_yaml_ng::from_str(yaml).expect("parse");
        assert_eq!(spec.socket_type, SocketType::Datagram);
    }

    #[test]
    fn socket_type_serializes_lowercase() {
        let json = serde_json::to_string(&SocketType::Sequential).expect("serialize");
        assert_eq!(json, "\"sequential\"");
    }
}
