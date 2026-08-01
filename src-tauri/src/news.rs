//! RSS news feed — port of the news section of `landing.js`.
//!
//! The feed URL comes from the distribution index's `rss` field. As with the
//! distribution itself, a plain path or `file://` URL is accepted as well as
//! `http(s)`, so a feed can be tested locally before pointing at a live one.

use std::path::PathBuf;

use serde::Serialize;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Article {
    pub title: String,
    pub link: String,
    pub author: String,
    pub date: String,
    /// Feed HTML. Rendered into the news pane, which is why the frontend
    /// treats it as trusted markup — see the note at the call site.
    pub content: String,
}

/// Fetch the feed body from wherever `source` points.
async fn read_source(source: &str) -> Result<String> {
    let s = source.trim();
    if s.starts_with("http://") || s.starts_with("https://") {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        Ok(client.get(s).send().await?.error_for_status()?.text().await?)
    } else {
        let path = PathBuf::from(s.strip_prefix("file://").unwrap_or(s));
        tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| Error::Other(format!("Could not read feed {}: {e}", path.display())))
    }
}

/// Parse an RSS 2.0 document into articles.
///
/// Written against the element names Helios relied on rather than a general
/// feed library: `<item>` with title/link/pubDate, plus `dc:creator` for the
/// author and `content:encoded` for the body, falling back to `description`.
pub fn parse_rss(xml: &str) -> Result<Vec<Article>> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut articles = Vec::new();
    let mut buf = Vec::new();

    let mut in_item = false;
    let mut current_tag = String::new();
    let (mut title, mut link, mut author, mut date, mut content, mut description) = (
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
    );

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "item" {
                    in_item = true;
                    title.clear();
                    link.clear();
                    author.clear();
                    date.clear();
                    content.clear();
                    description.clear();
                }
                current_tag = name;
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "item" && in_item {
                    in_item = false;
                    articles.push(Article {
                        title: std::mem::take(&mut title),
                        link: std::mem::take(&mut link),
                        author: std::mem::take(&mut author),
                        date: std::mem::take(&mut date),
                        content: if content.is_empty() {
                            std::mem::take(&mut description)
                        } else {
                            std::mem::take(&mut content)
                        },
                    });
                }
                current_tag.clear();
            }
            // Text and CDATA carry different types, so they are unescaped
            // separately. CDATA is taken verbatim — that is the point of it,
            // and feed bodies are HTML.
            Ok(ev @ (Event::Text(_) | Event::CData(_))) => {
                if !in_item {
                    buf.clear();
                    continue;
                }
                let text = match ev {
                    Event::Text(t) => t
                        .unescape()
                        .map(|c| c.into_owned())
                        .unwrap_or_else(|_| String::from_utf8_lossy(t.as_ref()).to_string()),
                    Event::CData(c) => String::from_utf8_lossy(c.as_ref()).to_string(),
                    _ => unreachable!(),
                };
                match current_tag.as_str() {
                    "title" => title.push_str(&text),
                    "link" => link.push_str(&text),
                    "dc:creator" | "creator" | "author" => author.push_str(&text),
                    "pubDate" => date.push_str(&text),
                    "content:encoded" | "encoded" => content.push_str(&text),
                    "description" => description.push_str(&text),
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(Error::Other(format!("Malformed RSS: {e}"))),
            _ => {}
        }
        buf.clear();
    }

    Ok(articles)
}

pub async fn load(source: &str) -> Result<Vec<Article>> {
    if source.trim().is_empty() {
        return Ok(Vec::new());
    }
    let body = read_source(source).await?;
    parse_rss(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:dc="http://purl.org/dc/elements/1.1/"
     xmlns:content="http://purl.org/rss/1.0/modules/content/">
  <channel>
    <title>Lunar News</title>
    <item>
      <title>Server maintenance</title>
      <link>https://example.com/1</link>
      <dc:creator>FlukRocker</dc:creator>
      <pubDate>Fri, 01 Aug 2025 10:00:00 +0000</pubDate>
      <content:encoded><![CDATA[<p>The server will be <b>down</b> briefly.</p>]]></content:encoded>
    </item>
    <item>
      <title>New season</title>
      <link>https://example.com/2</link>
      <pubDate>Thu, 31 Jul 2025 09:00:00 +0000</pubDate>
      <description>Plain description fallback.</description>
    </item>
  </channel>
</rss>"#;

    #[test]
    fn parses_items_with_creator_and_encoded_content() {
        let a = parse_rss(SAMPLE).unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].title, "Server maintenance");
        assert_eq!(a[0].link, "https://example.com/1");
        assert_eq!(a[0].author, "FlukRocker");
        assert!(a[0].date.contains("01 Aug 2025"));
        assert!(a[0].content.contains("<b>down</b>"), "CDATA body must survive");
    }

    #[test]
    fn falls_back_to_description_when_there_is_no_encoded_content() {
        let a = parse_rss(SAMPLE).unwrap();
        assert_eq!(a[1].content, "Plain description fallback.");
        assert_eq!(a[1].author, "", "missing creator is empty, not an error");
    }

    #[test]
    fn channel_level_title_is_not_mistaken_for_an_article() {
        let a = parse_rss(SAMPLE).unwrap();
        assert!(!a.iter().any(|x| x.title == "Lunar News"));
    }

    #[test]
    fn empty_and_itemless_feeds_yield_no_articles() {
        assert!(parse_rss("<rss><channel></channel></rss>").unwrap().is_empty());
    }

    #[test]
    fn malformed_xml_is_an_error_rather_than_a_panic() {
        assert!(parse_rss("<rss><channel><item><title>x</rss>").is_err());
    }

    #[tokio::test]
    async fn blank_source_is_not_an_error() {
        assert!(load("").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn reads_a_local_file() {
        let p = std::env::temp_dir().join("lunar-news-test.xml");
        tokio::fs::write(&p, SAMPLE).await.unwrap();
        let a = load(p.to_str().unwrap()).await.unwrap();
        assert_eq!(a.len(), 2);
        let _ = tokio::fs::remove_file(&p).await;
    }
}
