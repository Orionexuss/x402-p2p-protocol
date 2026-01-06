use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct MagnetLink {
    pub info_hash: String,
    pub display_name: Option<String>,
    pub trackers: Vec<String>,
    pub exact_length: Option<u64>,
}

impl MagnetLink {
    /// Parse a magnet URL string into a MagnetLink struct
    pub fn parse(url: &str) -> Result<Self, String> {
        if !url.starts_with("magnet:?") {
            return Err("Invalid magnet link: must start with 'magnet:?'".to_string());
        }

        let params_str = &url[8..]; // Skip "magnet:?"
        let params = parse_query_params(params_str);

        // Extract info hash from xt parameter
        let info_hash = params
            .get("xt")
            .and_then(|v| v.first())
            .ok_or("Missing required 'xt' parameter")?;

        println!("Extracted xt parameter: {}", info_hash);

        // Validate and extract the hash
        let info_hash = extract_info_hash(info_hash)?;

        // Extract display name
        let display_name = params
            .get("dn")
            .and_then(|v| v.first())
            .map(|s| url_decode(s));

        // Extract all tracker URLs
        let trackers = params
            .get("tr")
            .map(|v| v.iter().map(|s| url_decode(s)).collect())
            .unwrap_or_default();

        // Extract exact length
        let exact_length = params
            .get("xl")
            .and_then(|v| v.first())
            .and_then(|s| s.parse::<u64>().ok());

        Ok(MagnetLink {
            info_hash,
            display_name,
            trackers,
            exact_length,
        })
    }

    /// Convert the MagnetLink back to a magnet URL string
    pub fn to_url(&self) -> String {
        let mut url = format!("magnet:?xt=urn:btih:{}", self.info_hash);

        if let Some(ref name) = self.display_name {
            url.push_str(&format!("&dn={}", url_encode(name)));
        }

        for tracker in &self.trackers {
            url.push_str(&format!("&tr={}", url_encode(tracker)));
        }

        if let Some(length) = self.exact_length {
            url.push_str(&format!("&xl={}", length));
        }

        url
    }
}

/// Parse query parameters from a URL query string
fn parse_query_params(query: &str) -> HashMap<String, Vec<String>> {
    let mut params: HashMap<String, Vec<String>> = HashMap::new();

    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            params
                .entry(key.to_string())
                .or_insert_with(Vec::new)
                .push(value.to_string());
        }
    }

    params
}

/// Extract the info hash from an xt parameter value
fn extract_info_hash(xt: &str) -> Result<String, String> {
    // Expected format: urn:btih:<hash>
    if !xt.starts_with("urn:btih:") {
        return Err("Invalid xt parameter: must be 'urn:btih:<hash>'".to_string());
    }

    let hash = &xt[9..]; // Skip "urn:btih:"

    // Validate hash length (40 chars for hex, 32 for base32)
    if hash.len() != 40 && hash.len() != 32 {
        return Err(format!(
            "Invalid info hash length: expected 40 (hex) or 32 (base32), got {}",
            hash.len()
        ));
    }

    // Validate hex characters if 40 chars
    if hash.len() == 40 && !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Invalid info hash: not valid hexadecimal".to_string());
    }

    Ok(hash.to_lowercase())
}

/// Simple URL decode (percent-encoding)
fn url_decode(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars();

    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            } else {
                result.push(c);
                result.push_str(&hex);
            }
        } else if c == '+' {
            result.push(' ');
        } else {
            result.push(c);
        }
    }

    result
}

/// Simple URL encode (percent-encoding)
fn url_encode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "+".to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_magnet() {
        let url = "magnet:?xt=urn:btih:d2474e86c95b19b8bcfdb92bc12c9d44667cfa36";
        let magnet = MagnetLink::parse(url).unwrap();

        assert_eq!(magnet.info_hash, "d2474e86c95b19b8bcfdb92bc12c9d44667cfa36");
        assert_eq!(magnet.display_name, None);
        assert_eq!(magnet.trackers.len(), 0);
        assert_eq!(magnet.exact_length, None);
    }

    #[test]
    fn test_parse_full_magnet() {
        let url = "magnet:?xt=urn:btih:d2474e86c95b19b8bcfdb92bc12c9d44667cfa36&dn=Ubuntu+20.04&tr=udp://tracker.example.com:80&tr=http://tracker2.example.com&xl=2147483648";
        let magnet = MagnetLink::parse(url).unwrap();

        assert_eq!(magnet.info_hash, "d2474e86c95b19b8bcfdb92bc12c9d44667cfa36");
        assert_eq!(magnet.display_name, Some("Ubuntu 20.04".to_string()));
        assert_eq!(magnet.trackers.len(), 2);
        assert_eq!(magnet.trackers[0], "udp://tracker.example.com:80");
        assert_eq!(magnet.trackers[1], "http://tracker2.example.com");
        assert_eq!(magnet.exact_length, Some(2147483648));
    }

    #[test]
    fn test_to_url() {
        let magnet = MagnetLink {
            info_hash: "d2474e86c95b19b8bcfdb92bc12c9d44667cfa36".to_string(),
            display_name: Some("Test File".to_string()),
            trackers: vec!["udp://tracker.example.com:80".to_string()],
            exact_length: Some(1024),
        };

        let url = magnet.to_url();
        assert!(url.contains("xt=urn:btih:d2474e86c95b19b8bcfdb92bc12c9d44667cfa36"));
        assert!(url.contains("dn=Test+File"));
        // URL encoding converts : and / to %3A and %2F
        assert!(url.contains("tr=udp%3A%2F%2Ftracker.example.com%3A80"));
        assert!(url.contains("xl=1024"));
    }

    #[test]
    fn test_parse_invalid_magnet() {
        let result = MagnetLink::parse("http://example.com");
        assert!(result.is_err());

        let result = MagnetLink::parse("magnet:?dn=test");
        assert!(result.is_err());
    }

    #[test]
    fn test_roundtrip() {
        let original = MagnetLink {
            info_hash: "d2474e86c95b19b8bcfdb92bc12c9d44667cfa36".to_string(),
            display_name: Some("Test".to_string()),
            trackers: vec!["udp://tracker.test.com:80".to_string()],
            exact_length: Some(999),
        };

        let url = original.to_url();
        let parsed = MagnetLink::parse(&url).unwrap();

        assert_eq!(original, parsed);
    }
}
