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

/// Format seconds as M:SS or H:MM:SS if >= 1 hour.
fn format_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

pub struct FormatSpec {
    pub needs_elapsed: bool,
}

fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let url = Url::parse(uri).ok()?;
    url.to_file_path().ok()
}

/// Resolve album art into two clear outputs:
/// - art_url  → raw MPRIS value (unchanged)
/// - art_path → absolute filesystem path (cached or local)
fn resolve_art_outputs(s: &PlayerSnapshotOut) -> (String, String) {
    let art_url = s.art_url.clone().unwrap_or_default();

    if let Some(p) = &s.art_path {
        let path = p.display().to_string();
        trace!(path = %path, "Using resolved art_path from daemon");
        return (art_url, path);
    }

    // Safety fallback: if daemon didn't provide art_path but art_url is file://, derive it.
    if let Some(u) = s.art_url.as_deref()
        && u.starts_with("file://")
    {
        match file_uri_to_path(u) {
            Some(p) => {
                let path = p.display().to_string();
                trace!(path = %path, "Derived art_path from file:// art_url (fallback)");
                return (art_url, path);
            }
            None => {
                debug!(art_url = %u, "Failed to convert file:// URI to path (fallback)");
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

    let rate_s = s.rate.map(|v| v.to_string()).unwrap_or_default();
    let volume_s = s.volume.map(|v| v.to_string()).unwrap_or_default();

    let shuffle_s = match s.shuffle {
        Some(true) => "on",
        Some(false) => "off",
        None => "",
    };

    let loop_s = s.loop_status.as_deref().unwrap_or("");

    let (art_url, art_path) = resolve_art_outputs(s);

    let out = tpl
        .replace("{title}", s.title.as_deref().unwrap_or(""))
        .replace("{artist}", s.artist.as_deref().unwrap_or(""))
        .replace("{album}", s.album.as_deref().unwrap_or(""))
        .replace("{status}", &s.status)
        .replace("{player}", &s.player_id)
        .replace("{elapsed}", &elapsed)
        .replace("{length}", &length)
        .replace("{rate}", &rate_s)
        .replace("{volume}", &volume_s)
        .replace("{shuffle}", shuffle_s)
        .replace("{loop}", loop_s)
        .replace("{art_url}", &art_url)
        .replace("{art_path}", &art_path);

    trace!(output = %out, "Formatted template output");
    out
}
