// Copyright (C) 2025 Windblows2000
// This file is part of nexa.
//
// nexa is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use crate::ipc::PlayerSnapshotOut;
use std::path::PathBuf;
use tracing::{debug, trace};
use url::Url;

pub fn format_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let url = Url::parse(uri).ok()?;
    url.to_file_path().ok()
}

fn resolve_art_outputs(s: &PlayerSnapshotOut) -> (String, String) {
    let art_url = s.art_url.clone().unwrap_or_default();

    if let Some(path) = &s.art_path {
        let path = path.display().to_string();
        trace!(path = %path, "Using resolved art_path from daemon");
        return (art_url, path);
    }

    if let Some(uri) = s.art_url.as_deref()
        && uri.starts_with("file://")
    {
        match file_uri_to_path(uri) {
            Some(path) => {
                let path = path.display().to_string();
                trace!(path = %path, "Derived art_path from file:// art_url (fallback)");
                return (art_url, path);
            }
            None => {
                debug!(art_url = %uri, "Failed to convert file:// URI to path (fallback)");
            }
        }
    }

    trace!("No usable album art path available");
    (art_url, String::new())
}

pub fn format_template(tpl: &str, s: &PlayerSnapshotOut) -> String {
    trace!(
        player = %s.player_id,
        status = %s.status,
        "Formatting template"
    );

    let length = s.length.map(format_duration).unwrap_or_default();
    let elapsed = format_duration(s.elapsed);
    let rate = s.rate.map(|v| v.to_string()).unwrap_or_default();
    let volume = s.volume.map(|v| v.to_string()).unwrap_or_default();
    let shuffle = match s.shuffle {
        Some(true) => "on",
        Some(false) => "off",
        None => "",
    };
    let loop_status = s.loop_status.as_deref().unwrap_or("");
    let (art_url, art_path) = resolve_art_outputs(s);

    let replacement = |key: &str| -> Option<&str> {
        match key {
            "title" => Some(s.title.as_deref().unwrap_or("")),
            "artist" => Some(s.artist.as_deref().unwrap_or("")),
            "album" => Some(s.album.as_deref().unwrap_or("")),
            "status" => Some(&s.status),
            "player" => Some(&s.player_id),
            "elapsed" | "position" => Some(&elapsed),
            "length" => Some(&length),
            "rate" => Some(&rate),
            "volume" => Some(&volume),
            "shuffle" => Some(shuffle),
            "loop" => Some(loop_status),
            "art_url" => Some(&art_url),
            "art_path" => Some(&art_path),
            _ => None,
        }
    };

    let mut out = String::with_capacity(tpl.len() + 64);
    let mut rest = tpl;

    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        rest = &rest[open + 1..];

        let Some(close) = rest.find('}') else {
            out.push('{');
            out.push_str(rest);
            trace!(output = %out, "Formatted template output");
            return out;
        };

        let key = &rest[..close];
        if let Some(value) = replacement(key) {
            out.push_str(value);
        } else {
            out.push('{');
            out.push_str(key);
            out.push('}');
        }

        rest = &rest[close + 1..];
    }

    out.push_str(rest);
    trace!(output = %out, "Formatted template output");
    out
}
