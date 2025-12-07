use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderKind{
    Spotify,
    Youtube
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track{
    pub id : String,
    pub name : String,
    pub artists : Vec<String>,
    pub duration_ms : u64,
    pub provider : ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata : Option<serde_json::Value>
}

#[derive(Debug, Clone)]
pub enum TrackChange{
    Added {track : Track, index : usize},
    Removed {track : Track, index : usize},
    Moved {track : Track, from : usize, to : usize}
}

