// Copyright (C) 2025 Jelmer Vernooĳ <jelmer@jelmer.uk>
//
// This program is free software; you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation; either version 2 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program; if not, write to the Free Software
// Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA

//! Find a list of tags that might qualify for inclusion in
//! the fix_override_info function in lintian-brush/src/lintian_overrides.rs

use sqlx::Row;
use std::collections::{HashMap, HashSet};
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Connect to UDD mirror
    let pool = debian_analyzer::udd::connect_udd_mirror().await?;

    // Query for tags that appear in mismatched-override errors
    let query = "SELECT package_type, package, package_version, information \
                 FROM lintian WHERE tag = 'mismatched-override'";

    let rows = sqlx::query(query).fetch_all(&pool).await?;

    let mut tag_count: HashMap<String, usize> = HashMap::new();

    for row in &rows {
        let info: String = row.get("information");
        // Extract the tag name - it's the first word in the information field
        if let Some(tag) = info.split_whitespace().next() {
            *tag_count.entry(tag.to_string()).or_insert(0) += 1;
        }
    }

    // Query for tags that have location info (contain '[' in their information field)
    let location_query = "SELECT tag FROM lintian WHERE information LIKE '%[%'";
    let location_rows = sqlx::query(location_query).fetch_all(&pool).await?;

    let mut tags_with_location_info: HashSet<String> = HashSet::new();
    for row in &location_rows {
        let tag: String = row.get("tag");
        tags_with_location_info.insert(tag);
    }

    // Sort by count (descending) and print results
    let mut sorted_tags: Vec<_> = tag_count.into_iter().collect();
    sorted_tags.sort_by(|a, b| b.1.cmp(&a.1));

    for (tag, count) in sorted_tags {
        // Skip tags without location info
        if !tags_with_location_info.contains(&tag) {
            continue;
        }

        // Skip tags that already have fixers
        if lintian_brush::lintian_overrides::has_info_fixer(&tag) {
            continue;
        }

        println!("{:50}  {}", tag, count);
    }

    Ok(())
}
