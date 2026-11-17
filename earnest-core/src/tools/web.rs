use reqwest::blocking::Client;
use serde_json::Value;
use std::time::Duration;
use regex::Regex;

pub fn search_web(query: &str, max_results: usize) -> Result<Value, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()
        .map_err(|e| format!("Erreur initialisation client HTTP: {}", e))?;

    // 1. Bing RSS (XML structuré, sans challenge anti-bot JS/CAPTCHA)
    let encoded_q = urlencoding::encode(query);
    let rss_url = format!("https://www.bing.com/search?q={}&format=rss&mkt=fr-FR", encoded_q);
    if let Ok(resp) = client.get(&rss_url).send() {
        if resp.status().is_success() {
            if let Ok(xml) = resp.text() {
                let results = parse_bing_rss(&xml, max_results);
                if !results.is_empty() {
                    return Ok(serde_json::json!({
                        "status": "success",
                        "engine": "bing_rss",
                        "query": query,
                        "count": results.len(),
                        "results": results
                    }));
                }
            }
        }
    }

    // 2. Fallback Wikipedia OpenSearch API (JSON)
    let wiki_url = format!(
        "https://en.wikipedia.org/w/api.php?action=query&list=search&format=json&srlimit={}&srsearch={}",
        max_results.max(1),
        encoded_q
    );
    let mut results = Vec::new();
    if let Ok(resp) = client.get(&wiki_url).send() {
        if let Ok(data) = resp.json::<Value>() {
            let re_tags = Regex::new(r"<[^>]+>").unwrap();
            if let Some(arr) = data["query"]["search"].as_array() {
                for item in arr.iter().take(max_results) {
                    let title = item["title"].as_str().unwrap_or("").to_string();
                    let snippet = re_tags.replace_all(item["snippet"].as_str().unwrap_or(""), "").trim().to_string();
                    let url = format!("https://en.wikipedia.org/wiki/{}", title.replace(' ', "_"));
                    results.push(serde_json::json!({
                        "title": title,
                        "url": url,
                        "snippet": snippet
                    }));
                }
            }
        }
    }

    Ok(serde_json::json!({
        "status": "success",
        "engine": "wikipedia_fallback",
        "query": query,
        "count": results.len(),
        "results": results
    }))
}

fn parse_bing_rss(xml: &str, max_results: usize) -> Vec<Value> {
    let re_item = Regex::new(r"(?s)<item>(.*?)</item>").unwrap();
    let re_title = Regex::new(r"(?s)<title>(.*?)</title>").unwrap();
    let re_link = Regex::new(r"(?s)<link>(.*?)</link>").unwrap();
    let re_desc = Regex::new(r"(?s)<description>(.*?)</description>").unwrap();
    let unescape_xml = |s: &str| -> String {
        s.replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .trim()
            .to_string()
    };

    let mut results = Vec::new();
    for cap in re_item.captures_iter(xml).take(max_results) {
        let block = &cap[1];
        let title = re_title.captures(block).map(|c| unescape_xml(&c[1])).unwrap_or_default();
        let url = re_link.captures(block).map(|c| unescape_xml(&c[1])).unwrap_or_default();
        let snippet = re_desc.captures(block).map(|c| unescape_xml(&c[1])).unwrap_or_default();
        if !title.is_empty() || !url.is_empty() {
            results.push(serde_json::json!({
                "title": title,
                "url": url,
                "snippet": snippet
            }));
        }
    }
    results
}

pub fn fetch_web_page(url: &str, max_chars: usize) -> Result<Value, String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("Seules les URLs HTTP/HTTPS sont autorisées.".to_string());
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()
        .map_err(|e| format!("Erreur initialisation client HTTP: {}", e))?;

    let resp = client.get(url).send().map_err(|e| format!("Erreur téléchargement page: {}", e))?;
    let status = resp.status().as_u16();
    let text = resp.text().map_err(|e| format!("Erreur lecture contenu: {}", e))?;

    // Nettoyage rapide des balises HTML
    let re_scripts = Regex::new(r"(?is)<script[^>]*>.*?</script>|<style[^>]*>.*?</style>").unwrap();
    let no_scripts = re_scripts.replace_all(&text, " ");
    let re_tags = Regex::new(r"<[^>]+>").unwrap();
    let clean_text = re_tags.replace_all(&no_scripts, " ").split_whitespace().collect::<Vec<_>>().join(" ");

    let char_count = clean_text.chars().count();
    let truncated = char_count > max_chars;
    let final_content = if truncated {
        let prefix: String = clean_text.chars().take(max_chars).collect();
        format!("{}... [Tronqué à {} caractères]", prefix, max_chars)
    } else {
        clean_text
    };

    Ok(serde_json::json!({
        "status_code": status,
        "truncated": truncated,
        "content": final_content
    }))
}

#[cfg(test)]
mod tests {
    use super::parse_bing_rss;

    #[test]
    fn parse_bing_rss_extracts_items() {
        let xml = r#"<rss><channel><item><title>FLUX.2 &amp; Dev</title><link>https://bfl.ai/flux2</link><description>32B flow matching model</description></item></channel></rss>"#;
        let items = parse_bing_rss(xml, 5);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["title"], "FLUX.2 & Dev");
        assert_eq!(items[0]["url"], "https://bfl.ai/flux2");
    }
}
