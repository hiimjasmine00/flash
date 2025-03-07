use super::builder::Builder;
use super::shared::fmt_emoji;
use super::traits::Entry;
use crate::html::{Html, HtmlElement, HtmlText};
use crate::lookahead::{CachedLookahead, CreateCachedLookahead};
use crate::url::UrlPath;
use pulldown_cmark::{CowStr, Event, LinkType, Tag};
use serde::{Deserialize, Deserializer};

#[derive(Clone, PartialEq, Default)]
pub enum Style {
    #[default]
    Default,
    QnA,
}

fn parse_style<'de, D>(deserializer: D) -> Result<Style, D::Error>
where
    D: Deserializer<'de>,
{
    match String::deserialize(deserializer)?.as_str() {
        "default" => Ok(Style::Default),
        "qna" => Ok(Style::QnA),
        _ => Err(serde::de::Error::custom("Invalid style")),
    }
}

#[derive(Deserialize, Clone, Default)]
pub struct Metadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub order: Option<usize>,
    #[serde(default = "Style::default", deserialize_with = "parse_style")]
    pub style: Style,
}

impl Metadata {
    pub fn new_with_title(title: String) -> Self {
        Self {
            title: Some(title),
            ..Default::default()
        }
    }
}

fn parse_markdown_metadata(doc: &str) -> (&str, Option<Metadata>) {
    // if the document has no metadata just parse it as markdown
    if !doc.trim_start().starts_with("---") {
        return (doc, None);
    }

    let doc = doc.trim_start().strip_prefix("---").unwrap();

    // make sure metadata ends properly
    let Some(metadata_end) = doc.find("---") else {
        return (doc, None);
    };
    let metadata_str = &doc[..metadata_end];

    // parse metadata
    (
        &doc[metadata_end + 3..],
        serde_yaml::from_str(metadata_str).expect("Invalid metadata in markdown"),
    )
}

#[derive(PartialEq)]
enum InsertP {
    Dont,
    Start,
    ToEnd,
}

struct MDStream<'i, 'c, 'b, 'e, const SIZE: usize, F: Fn(UrlPath) -> Option<UrlPath>> {
    iter: CachedLookahead<pulldown_cmark::Parser<'i, 'c>, SIZE>,
    url_fixer: Option<F>,
    builder: &'b Builder<'e>,
    metadata: Option<Metadata>,
    insert_para_stage: InsertP,
    inside_code_block: bool,
}

impl<'i, 'c, 'b, 'e, const SIZE: usize, F: Fn(UrlPath) -> Option<UrlPath>>
    MDStream<'i, 'c, 'b, 'e, SIZE, F>
{
    pub fn new(
        iter: pulldown_cmark::Parser<'i, 'c>,
        url_fixer: Option<F>,
        builder: &'b Builder<'e>,
        metadata: Option<Metadata>,
    ) -> MDStream<'i, 'c, 'b, 'e, SIZE, F> {
        MDStream {
            iter: iter.lookahead_cached::<SIZE>(),
            url_fixer,
            builder,
            metadata,
            insert_para_stage: InsertP::Dont,
            inside_code_block: false,
        }
    }
}

impl<'i, 'c, 'b, 'e, const SIZE: usize, F: Fn(UrlPath) -> Option<UrlPath>> Iterator
    for MDStream<'i, 'c, 'b, 'e, SIZE, F>
{
    type Item = Event<'i>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.insert_para_stage == InsertP::Start {
            self.insert_para_stage = InsertP::ToEnd;
            return Some(Event::Start(Tag::BlockQuote));
        } else if self.insert_para_stage == InsertP::ToEnd
            && match self.iter.peek() {
                Some(Event::Start(Tag::Heading(lvl, _, _))) => (*lvl as usize) == 2,
                None => true,
                _ => false,
            }
        {
            self.insert_para_stage = InsertP::Dont;
            return Some(Event::End(Tag::BlockQuote));
        }
        let event = self.iter.next()?;
        Some(match event {
            // Don't format emojis inside code blocks lol
            Event::Text(t) => {
                if self.inside_code_block {
                    Event::Text(t)
                } else {
                    Event::Text(CowStr::Boxed(Box::from(fmt_emoji(&t).as_str())))
                }
            }
            Event::Start(tag) => Event::Start(match tag {
                // Fix urls to point to root
                Tag::Link(ty, ref dest, ref title) | Tag::Image(ty, ref dest, ref title) => {
                    let mut new_dest;
                    if ty == LinkType::Inline
                        && dest.starts_with("/")
                        && let Some(ref url_fixer) = self.url_fixer
                    {
                        let url = UrlPath::new_with_path(
                            dest.split("/").map(|s| s.to_string()).collect(),
                        );
                        if let Some(url) = url_fixer(url) {
                            new_dest = url.to_unencoded_string();
                        } else {
                            new_dest = dest.to_string();
                        }
                    } else {
                        new_dest = dest.to_string();
                    }

                    // make the url absolute in any case if it starts with /
                    if dest.starts_with("/")
                        && let Ok(dest) = UrlPath::parse(&new_dest)
                    {
                        new_dest = dest
                            .to_absolute(self.builder.config.clone())
                            .to_unencoded_string();
                    }

                    // return fixed url
                    if matches!(tag, Tag::Link(_, _, _)) {
                        Tag::Link(ty, CowStr::Boxed(Box::from(new_dest)), title.to_owned())
                    } else {
                        Tag::Image(ty, CowStr::Boxed(Box::from(new_dest)), title.to_owned())
                    }
                }
                // Add id to heading so they can be navigated to with url#header
                Tag::Heading(lvl, mut frag, mut classes) => {
                    if frag.is_none() && (lvl as usize) < 4 {
                        let mut buf = String::new();
                        for t in self.iter.lookahead() {
                            match t {
                                Some(Event::Text(t)) => {
                                    if !buf.is_empty() {
                                        buf += " ";
                                    }
                                    // all text must be lowercase
                                    buf += &t
                                        .to_string()
                                        .chars()
                                        // no punctuation
                                        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
                                        .collect::<String>()
                                        .to_lowercase();
                                }
                                Some(Event::End(Tag::Heading(_, _, _))) => break,
                                // non-text is removed
                                _ => {}
                            }
                        }
                        // replace spaces with single hyphens
                        buf = buf.split_whitespace().collect::<Vec<_>>().join("-");

                        frag = Some(CowStr::Boxed(Box::from(buf)));
                    }
                    if let Some(ref meta) = self.metadata
                        && meta.style == Style::QnA
                        && (lvl as usize) < 3
                    {
                        classes.push(CowStr::Boxed(Box::from("qna-question")));
                    }
                    Tag::Heading(lvl, frag, classes)
                }
                Tag::CodeBlock(b) => {
                    self.inside_code_block = true;
                    Tag::CodeBlock(b)
                }
                _ => tag,
            }),
            Event::End(tag) => Event::End(match tag {
                Tag::Heading(lvl, frag, classes) => {
                    if let Some(ref meta) = self.metadata
                        && meta.style == Style::QnA
                        && (lvl as usize) == 2
                    {
                        self.insert_para_stage = InsertP::Start;
                    }
                    Tag::Heading(lvl, frag, classes)
                }
                Tag::CodeBlock(b) => {
                    self.inside_code_block = false;
                    Tag::CodeBlock(b)
                }
                _ => tag,
            }),
            _ => event,
        })
    }
}

#[allow(clippy::ptr_arg)]
pub fn fmt_markdown<F: Fn(UrlPath) -> Option<UrlPath>>(
    builder: &Builder,
    text: &str,
    url_fixer: Option<F>,
) -> Html {
    // skip metadata
    let (text, meta) = parse_markdown_metadata(text);

    // pulldown_cmark doesn't automatically generate header links for me, and I
    // need those to be able to have docs links. Unfortunately the mechanism it
    // provides for adding header links takes a &str and not an owned String, so
    // I have to do this to have Strings with the same lifetime as the input text

    let parser = MDStream::<5, F>::new(
        pulldown_cmark::Parser::new_ext(text, pulldown_cmark::Options::all()),
        url_fixer,
        builder,
        meta,
    );

    let mut content = String::new();
    pulldown_cmark::html::push_html(&mut content, parser);

    HtmlElement::new("div")
        .with_class("text")
        .with_child(Html::Raw(content))
        .into()
}

#[allow(clippy::ptr_arg)]
pub fn extract_metadata_from_md(text: &String, default_title: Option<String>) -> Option<Metadata> {
    let (text, metadata) = parse_markdown_metadata(text);

    // if the metadata provided the title, no need to parse the doc for it
    if metadata.is_some() && metadata.as_ref().unwrap().title.is_some() {
        return metadata;
    }

    // otherwise parse doc and use first header as title
    let mut parser = pulldown_cmark::Parser::new_ext(text, pulldown_cmark::Options::all());

    let name = parser.next()?;
    let Event::Start(tag) = name else { return None };
    let Tag::Heading(_, _, _) = tag else {
        return None;
    };

    let mut res = String::new();

    while match parser.next() {
        Some(ev) => match ev {
            Event::End(tag) => !matches!(tag, Tag::Heading(_, _, _)),
            Event::Text(text) => {
                res.push_str(&text);
                true
            }
            _ => true,
        },
        None => false,
    } {}

    // if some metadata was found, set the title
    if let Some(mut metadata) = metadata {
        metadata.title = (!res.is_empty()).then_some(res).or(default_title);
        Some(metadata)
    }
    // otherwise only return Some if a title was found
    else if res.is_empty() {
        default_title.map(Metadata::new_with_title)
    } else {
        Some(Metadata::new_with_title(res))
    }
}

pub fn output_tutorial<'e, T: Entry<'e>>(
    entry: &T,
    builder: &Builder,
    content: &str,
    links: Html,
) -> Vec<(&'static str, Html)> {
    vec![
        ("title", HtmlText::new(entry.name()).into()),
        (
            "content",
            fmt_markdown(
                builder,
                content,
                Some(|url: UrlPath| Some(url.remove_extension(".md"))),
            ),
        ),
        ("links", links),
    ]
}
