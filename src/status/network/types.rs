//! Serialized network structs.

use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct WifiNetwork {
    pub ssid: String,
    pub bssid: String,
    pub signal: i32,
    pub rate: String,
    pub security: String,
    pub active: bool,
    pub saved: bool,
    pub autoconnect: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct VpnConnection {
    pub name: String,
    pub vpn_type: String,
    pub device: String,
    pub active: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct ConnectionDetails {
    pub ip_address: String,
    pub gateway: String,
    pub dns: String,
    pub subnet: String,
    pub security: String,
    pub bssid: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct NetworkStatus {
    pub wifi_enabled: bool,
    pub airplane_mode: bool,
    pub connected: bool,
    pub active_ssid: String,
    pub active_signal: i32,
    pub active_speed: String,
    pub wifi_device: String,
    pub warp_connected: bool,
    pub tailscale_connected: bool,
    pub tailscale_ip: String,
    pub details: ConnectionDetails,
    pub networks: Vec<WifiNetwork>,
    pub vpns: Vec<VpnConnection>,
}
