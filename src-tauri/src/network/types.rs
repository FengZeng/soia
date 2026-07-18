use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NetworkBrowseEntry {
    pub name: String,
    pub path: String,
    pub entry_type: String,
    pub playback_key: Option<String>,
    pub size: Option<u64>,
    pub modified_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NetworkBrowseResult {
    pub path: String,
    pub entries: Vec<NetworkBrowseEntry>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscoveredNetworkConnection {
    pub protocol: String,
    pub usn: Option<String>,
    pub location: String,
    pub friendly_name: Option<String>,
    pub server: Option<String>,
    pub st: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscoverNetworkPayload {
    pub protocol: Option<String>,
    pub timeout_secs: Option<u64>,
    pub scan_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowseNetworkPayload {
    pub connection_id: String,
    pub mode: String,
    pub protocol: Option<String>,
    pub path: Option<String>,
}
