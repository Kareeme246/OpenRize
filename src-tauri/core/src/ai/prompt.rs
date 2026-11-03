//! The one classification prompt, output schema and reply validator every
//! language-model provider shares.
//!
//! The Swift sidecar (`swift/Sources/openrize-ml/Language.swift`) still
//! builds its own copy of this prompt for Apple's model; the two must stay in
//! sync until the Swift side takes it from its classify params. Pure logic,
//! no I/O.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::arbiter::{Label, Votes};

/// Bump on any wording change; it is part of `model_version`.
pub const PROMPT_VERSION: u32 = 1;
/// Label used for "not part of any listed project". A real answer with its
/// own votes, not a missing value.
pub const NO_PROJECT: &str = "No project";
pub const MAX_DESCRIPTION_WORDS: usize = 15;
pub const MAX_DESCRIPTION_CHARS: usize = 120;

const DESCRIPTION_FIELD_HELP: &str = "A short name for the block, 3 to 15 words, starting with a past-tense verb and naming one specific thing, optionally with what it was for. The form, not the content, of names like: \"Prepared the 2.4 release\", \"Researched standing desk options\", \"Wrote the onboarding guide\", \"Answered client email\", \"Watched YouTube\".";

const INSTRUCTIONS: &str =
    "You name and file blocks of computer time for a private, on-device time tracker. \
The person reads these names later to recall their day, so a name says what the block \
was for, the way they would say it to a colleague.
Each block lists the apps used and their share of the time, then window titles or sites.
Find the one thread of work that the block was for and name only that. Leave out brief \
or incidental things: a quick look at email, a stray tab, the time tracker itself. \
Mention a second thing only when it took about as much time as the first.
Work comes before background media. When a terminal or editor shows a project for a \
real part of the block while videos or streams fill the browser, the project is the \
work: name it. Name the kind of activity, like \"Watched YouTube\", only when the block \
is mostly videos or browsing with no project work, and never list video topics.
Be specific: name the project, feature, document, or site, not the app. Never write \
\"Used Safari and Terminal\". Never mention a computer name, a folder path, or what came \
before the block. Only use details that appear in the block.
Choose the category that fits that main thread.";

const PROJECT_INSTRUCTIONS: &str =
    "\nChoose the project the work belongs to. Answer \"No project\" unless the \
main thread clearly matches one of the listed projects.";

/// A category or project the model may choose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// Category `ai_prompt` / project `ai_hints`.
    pub hint: Option<String>,
}

/// A block the user already labeled. Names, not ids.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Example {
    pub features: String,
    pub category: String,
    pub project: Option<String>,
}

/// Everything one classification sends to a language model.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmRequest {
    pub features: String,
    pub categories: Vec<Choice>,
    /// `None` when only a category is wanted.
    pub projects: Option<Vec<Choice>>,
    pub examples: Vec<Example>,
    /// `settings.ai_custom_prompt`, possibly empty.
    pub custom: String,
}

pub struct Prompt {
    pub system: String,
    pub user: String,
}

/// Distinct display names, same order as `choices`: trimmed, and the 2nd,
/// 3rd... duplicate get " (2)", " (3)".
pub fn unique_names(choices: &[Choice]) -> Vec<String> {
    let mut seen: HashMap<&str, u32> = HashMap::new();
    choices
        .iter()
        .map(|choice| {
            let base = choice.name.trim();
            let count = seen.entry(base).or_insert(0);
            *count += 1;
            if *count == 1 {
                base.to_string()
            } else {
                format!("{base} ({count})")
            }
        })
        .collect()
}

fn detail(choice: &Choice) -> String {
    let parts: Vec<&str> = [choice.description.as_deref(), choice.hint.as_deref()]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!(": {}", parts.join(" "))
    }
}

fn list_section(text: &mut String, title: &str, choices: &[Choice]) {
    text.push_str("\n\n");
    text.push_str(title);
    for (choice, name) in choices.iter().zip(unique_names(choices)) {
        text.push_str("\n- ");
        text.push_str(&name);
        text.push_str(&detail(choice));
    }
}

/// The system prompt (Swift `instructions()`) and the user prompt (Swift
/// `prompt()`).
pub fn build(request: &LlmRequest) -> Prompt {
    let mut system = String::from(INSTRUCTIONS);
    if request.projects.is_some() {
        system.push_str(PROJECT_INSTRUCTIONS);
    }
    list_section(&mut system, "Categories:", &request.categories);
    if let Some(projects) = &request.projects {
        list_section(&mut system, "Projects:", projects);
    }
    let custom = request.custom.trim();
    if !custom.is_empty() {
        system.push_str("\n\nThe user's own instructions:\n");
        system.push_str(custom);
    }

    let mut user = String::new();
    if !request.examples.is_empty() {
        user.push_str("Blocks the user already labeled:\n");
        for example in &request.examples {
            user.push_str(&format!(
                "\n{}\n=> Category: {}",
                example.features, example.category
            ));
            if request.projects.is_some() {
                user.push_str(&format!(
                    ", Project: {}",
                    example.project.as_deref().unwrap_or(NO_PROJECT)
                ));
            }
            user.push('\n');
        }
        user.push('\n');
    }
    user.push_str("Block to label:\n");
    user.push_str(&request.features);

    Prompt { system, user }
}

fn project_names(request: &LlmRequest) -> Option<Vec<String>> {
    request.projects.as_deref().map(unique_names)
}

/// The JSON Schema for `response_format: {type: "json_schema", json_schema:
/// {name: "ActivityLabel", strict: true, schema: <this>}}`. OpenAI
/// strict-mode compatible: flat, all keys required, no extra properties.
pub fn json_schema(request: &LlmRequest) -> Value {
    let mut properties = Map::new();
    properties.insert(
        "description".into(),
        json!({"type": "string", "description": DESCRIPTION_FIELD_HELP}),
    );
    properties.insert(
        "category".into(),
        json!({
            "type": "string",
            "description": "The category that best describes the work.",
            "enum": unique_names(&request.categories),
        }),
    );
    let mut required = vec!["description", "category"];
    if let Some(mut names) = project_names(request) {
        names.push(NO_PROJECT.to_string());
        properties.insert(
            "project".into(),
            json!({
                "type": "string",
                "description": format!("The project this work belongs to, or \"{NO_PROJECT}\"."),
                "enum": names,
            }),
        );
        required.push("project");
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

/// Extra user-prompt text for servers that reject `json_schema` and fall
/// back to `response_format: {type: "json_object"}`.
pub fn json_object_hint(request: &LlmRequest) -> String {
    let keys = if request.projects.is_some() {
        "description, category, project"
    } else {
        "description, category"
    };
    let mut text = format!(
        "Answer with only a JSON object, no other text, with the keys {keys}.\ncategory must be exactly one of: {}.",
        unique_names(&request.categories).join(" | ")
    );
    if let Some(mut names) = project_names(request) {
        names.push(NO_PROJECT.to_string());
        text.push_str(&format!(
            "\nproject must be exactly one of: {}.",
            names.join(" | ")
        ));
    }
    text
}

/// One generation, validated and mapped back to ids.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sample {
    /// Category id; `None` = no category vote.
    pub category: Option<String>,
    /// `None` when projects were not requested; `Some(None)` = "No project"
    /// (also for unknown or missing names); `Some(Some(id))` otherwise.
    pub project: Option<Option<String>>,
    /// Trimmed description if it passes the rules, else `None`.
    pub description: Option<String>,
}

/// Remove `<think>...</think>` blocks (and an orphan leading `</think>`).
fn strip_think(text: &str) -> Result<String, String> {
    const OPEN: &str = "<think>";
    const CLOSE: &str = "</think>";
    let mut rest = text;
    let mut out = String::new();
    loop {
        let open = rest.find(OPEN);
        let close = rest.find(CLOSE);
        match (open, close) {
            (Some(o), Some(c)) if c > o => {
                out.push_str(&rest[..o]);
                rest = &rest[c + CLOSE.len()..];
            }
            // A close tag with no opener before it: drop everything up to it.
            (_, Some(c)) => rest = &rest[c + CLOSE.len()..],
            (Some(_), None) => {
                return Err("the reply ended inside an unterminated <think> block".to_string());
            }
            (None, None) => {
                out.push_str(rest);
                return Ok(out);
            }
        }
    }
}

/// End index (exclusive) of the balanced `{...}` starting at `start`, aware
/// of strings and escapes.
fn balanced_end(text: &str, start: usize) -> Option<usize> {
    let mut depth = 0u32;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, ch) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + offset + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn first_json_object(text: &str) -> Option<Map<String, Value>> {
    text.match_indices('{').find_map(|(start, _)| {
        let end = balanced_end(text, start)?;
        match serde_json::from_str::<Value>(&text[start..end]) {
            Ok(Value::Object(map)) => Some(map),
            _ => None,
        }
    })
}

fn id_for(choices: &[Choice], names: &[String], wanted: Option<&str>) -> Option<String> {
    let wanted = wanted?.trim();
    names
        .iter()
        .position(|name| name == wanted)
        .map(|index| choices[index].id.clone())
}

fn description_ok(text: &str) -> Option<String> {
    let text = text.trim();
    let ok = !text.is_empty()
        && text.split_whitespace().count() <= MAX_DESCRIPTION_WORDS
        && text.chars().count() <= MAX_DESCRIPTION_CHARS;
    ok.then(|| text.to_string())
}

/// Never trust the server. Strips `<think>` blocks, takes the first
/// balanced JSON object (so prose or a code fence around it is fine), and
/// maps category and project names back to ids. Errors only when no JSON
/// object can be parsed.
pub fn parse_reply(text: &str, request: &LlmRequest) -> Result<Sample, String> {
    let cleaned = strip_think(text)?;
    let object = first_json_object(&cleaned)
        .ok_or_else(|| "the reply did not contain a JSON object".to_string())?;
    let get = |key: &str| object.get(key).and_then(Value::as_str);

    let category_names = unique_names(&request.categories);
    let category = id_for(&request.categories, &category_names, get("category"));
    let project = request.projects.as_deref().map(|projects| {
        let names = unique_names(projects);
        id_for(projects, &names, get("project"))
    });
    let description = get("description").and_then(description_ok);
    Ok(Sample {
        category,
        project,
        description,
    })
}

/// Counts samples into votes the way the Swift sidecar does. Project votes
/// are returned only when projects were requested.
pub fn tally(samples: &[Sample], want_project: bool) -> (Votes, Option<Votes>, Option<String>) {
    let total = samples.len() as u32;
    let mut categories = Votes {
        counts: HashMap::new(),
        samples: total,
    };
    let mut projects = Votes {
        counts: HashMap::new(),
        samples: total,
    };
    for sample in samples {
        if let Some(id) = &sample.category {
            *categories.counts.entry(Some(id.clone())).or_insert(0) += 1;
        }
        if want_project {
            let label: Label = sample.project.clone().flatten();
            *projects.counts.entry(label).or_insert(0) += 1;
        }
    }
    let description = samples.iter().find_map(|sample| sample.description.clone());
    (categories, want_project.then_some(projects), description)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(id: &str, name: &str, description: Option<&str>, hint: Option<&str>) -> Choice {
        Choice {
            id: id.into(),
            name: name.into(),
            description: description.map(Into::into),
            hint: hint.map(Into::into),
        }
    }

    fn request(with_projects: bool) -> LlmRequest {
        LlmRequest {
            features: "Apps: Xcode 80%\nTitles: main.rs".into(),
            categories: vec![
                choice(
                    "c1",
                    "Coding",
                    Some(" Writing software "),
                    Some("repos, editors"),
                ),
                choice("c2", "Email", None, None),
            ],
            projects: with_projects.then(|| {
                vec![
                    choice("p1", "Alpha", None, Some("alpha-repo")),
                    choice("p2", "Alpha", Some("Second"), None),
                ]
            }),
            examples: vec![Example {
                features: "Apps: Mail 90%".into(),
                category: "Email".into(),
                project: None,
            }],
            custom: "  Be terse.  ".into(),
        }
    }

    const BASE: &str = "You name and file blocks of computer time for a private, on-device time tracker. The person reads these names later to recall their day, so a name says what the block was for, the way they would say it to a colleague.\n\
Each block lists the apps used and their share of the time, then window titles or sites.\n\
Find the one thread of work that the block was for and name only that. Leave out brief or incidental things: a quick look at email, a stray tab, the time tracker itself. Mention a second thing only when it took about as much time as the first.\n\
Work comes before background media. When a terminal or editor shows a project for a real part of the block while videos or streams fill the browser, the project is the work: name it. Name the kind of activity, like \"Watched YouTube\", only when the block is mostly videos or browsing with no project work, and never list video topics.\n\
Be specific: name the project, feature, document, or site, not the app. Never write \"Used Safari and Terminal\". Never mention a computer name, a folder path, or what came before the block. Only use details that appear in the block.\n\
Choose the category that fits that main thread.";

    #[test]
    fn build_with_projects() {
        let prompt = build(&request(true));
        let expected_system = format!(
            "{BASE}\nChoose the project the work belongs to. Answer \"No project\" unless the main thread clearly matches one of the listed projects.\n\nCategories:\n- Coding: Writing software repos, editors\n- Email\n\nProjects:\n- Alpha: alpha-repo\n- Alpha (2): Second\n\nThe user's own instructions:\nBe terse."
        );
        assert_eq!(prompt.system, expected_system);
        assert_eq!(
            prompt.user,
            "Blocks the user already labeled:\n\nApps: Mail 90%\n=> Category: Email, Project: No project\n\nBlock to label:\nApps: Xcode 80%\nTitles: main.rs"
        );
    }

    #[test]
    fn build_without_projects() {
        let mut req = request(false);
        req.custom = "   ".into();
        req.examples.clear();
        let prompt = build(&req);
        assert_eq!(
            prompt.system,
            format!("{BASE}\n\nCategories:\n- Coding: Writing software repos, editors\n- Email")
        );
        assert_eq!(
            prompt.user,
            "Block to label:\nApps: Xcode 80%\nTitles: main.rs"
        );

        let mut with_example = request(false);
        with_example.custom.clear();
        assert_eq!(
            build(&with_example).user,
            "Blocks the user already labeled:\n\nApps: Mail 90%\n=> Category: Email\n\nBlock to label:\nApps: Xcode 80%\nTitles: main.rs"
        );
    }

    #[test]
    fn unique_names_suffix_duplicates() {
        let choices = vec![
            choice("1", " A ", None, None),
            choice("2", "B", None, None),
            choice("3", "A", None, None),
            choice("4", "A", None, None),
        ];
        assert_eq!(unique_names(&choices), vec!["A", "B", "A (2)", "A (3)"]);
    }

    #[test]
    fn schema_shape() {
        let with = json_schema(&request(true));
        assert_eq!(with["type"], "object");
        assert_eq!(with["additionalProperties"], false);
        assert_eq!(
            with["required"],
            json!(["description", "category", "project"])
        );
        assert_eq!(with["properties"]["description"]["type"], "string");
        assert_eq!(
            with["properties"]["description"]["description"],
            DESCRIPTION_FIELD_HELP
        );
        assert_eq!(
            with["properties"]["category"]["enum"],
            json!(["Coding", "Email"])
        );
        assert_eq!(
            with["properties"]["category"]["description"],
            "The category that best describes the work."
        );
        assert_eq!(
            with["properties"]["project"]["enum"],
            json!(["Alpha", "Alpha (2)", "No project"])
        );
        assert_eq!(
            with["properties"]["project"]["description"],
            "The project this work belongs to, or \"No project\"."
        );

        let without = json_schema(&request(false));
        assert_eq!(without["required"], json!(["description", "category"]));
        assert!(without["properties"].get("project").is_none());
        assert_eq!(without["additionalProperties"], false);
    }

    #[test]
    fn json_object_hint_lists_names() {
        assert_eq!(
            json_object_hint(&request(true)),
            "Answer with only a JSON object, no other text, with the keys description, category, project.\ncategory must be exactly one of: Coding | Email.\nproject must be exactly one of: Alpha | Alpha (2) | No project."
        );
        assert_eq!(
            json_object_hint(&request(false)),
            "Answer with only a JSON object, no other text, with the keys description, category.\ncategory must be exactly one of: Coding | Email."
        );
    }

    fn parse(text: &str) -> Result<Sample, String> {
        parse_reply(text, &request(true))
    }

    #[test]
    fn parses_plain_json() {
        let sample = parse(
            r#"{"description":" Wrote the parser ","category":" Coding ","project":"Alpha (2)"}"#,
        )
        .unwrap();
        assert_eq!(sample.category.as_deref(), Some("c1"));
        assert_eq!(sample.project, Some(Some("p2".to_string())));
        assert_eq!(sample.description.as_deref(), Some("Wrote the parser"));
    }

    #[test]
    fn strips_think_blocks() {
        let sample = parse(
            "<think>maybe {\"category\":\"Email\"}</think>\n{\"description\":\"Read mail\",\"category\":\"Email\",\"project\":\"Alpha\"}",
        )
        .unwrap();
        assert_eq!(sample.category.as_deref(), Some("c2"));
        assert_eq!(sample.project, Some(Some("p1".to_string())));
    }

    #[test]
    fn finds_json_in_prose_and_fences() {
        let sample = parse(
            "Sure! Here you go:\n```json\n{\"description\":\"Read mail\",\"category\":\"Email\",\"project\":\"No project\"}\n```\nHope it helps {x}",
        )
        .unwrap();
        assert_eq!(sample.category.as_deref(), Some("c2"));
        assert_eq!(sample.project, Some(None));
    }

    #[test]
    fn braces_inside_strings() {
        let sample = parse(
            r#"{"description":"Fixed a } and \" { bug","category":"Coding","project":"No project"}"#,
        )
        .unwrap();
        assert_eq!(
            sample.description.as_deref(),
            Some("Fixed a } and \" { bug")
        );
        assert_eq!(sample.category.as_deref(), Some("c1"));
    }

    #[test]
    fn unknown_names() {
        let sample =
            parse(r#"{"description":"x","category":"Gardening","project":"Nope"}"#).unwrap();
        assert_eq!(sample.category, None);
        assert_eq!(sample.project, Some(None));
        let missing = parse(r#"{"description":"x"}"#).unwrap();
        assert_eq!(missing.category, None);
        assert_eq!(missing.project, Some(None));
        let no_projects = parse_reply(r#"{"category":"Coding"}"#, &request(false)).unwrap();
        assert_eq!(no_projects.project, None);
    }

    #[test]
    fn real_project_named_no_project_wins() {
        let mut req = request(true);
        req.projects = Some(vec![choice("p9", "No project", None, None)]);
        let sample = parse_reply(r#"{"category":"Coding","project":"No project"}"#, &req).unwrap();
        assert_eq!(sample.project, Some(Some("p9".to_string())));
    }

    #[test]
    fn bad_descriptions_are_dropped() {
        let long_words = vec!["word"; 16].join(" ");
        let long_chars = "a".repeat(121);
        let ok_words = vec!["word"; 15].join(" ");
        let multibyte = "é".repeat(120);
        for (text, kept) in [
            (long_words, false),
            (long_chars, false),
            (String::new(), false),
            ("   ".to_string(), false),
            (ok_words, true),
            (multibyte, true),
        ] {
            let reply = json!({"description": text, "category": "Coding"}).to_string();
            let sample = parse(&reply).unwrap();
            assert_eq!(sample.description.is_some(), kept, "{text:?}");
        }
    }

    #[test]
    fn unparseable_replies_error() {
        assert!(parse("I cannot do that").is_err());
        assert!(parse("{not json}").is_err());
        assert!(parse("{\"category\": \"Coding\"").is_err());
        assert!(parse("[1, 2]").is_err());
        assert!(parse("<think>still thinking {\"category\":\"Coding\"}").is_err());
    }

    #[test]
    fn tally_counts_votes() {
        let samples = vec![
            Sample {
                category: Some("c1".into()),
                project: Some(Some("p1".into())),
                description: None,
            },
            Sample {
                category: Some("c1".into()),
                project: Some(None),
                description: Some("First name".into()),
            },
            Sample {
                category: None,
                project: Some(None),
                description: Some("Second name".into()),
            },
        ];
        let (categories, projects, description) = tally(&samples, false);
        assert_eq!(categories.samples, 3);
        assert_eq!(categories.counts.len(), 1);
        assert_eq!(categories.counts[&Some("c1".to_string())], 2);
        assert!(projects.is_none());
        assert_eq!(description.as_deref(), Some("First name"));

        let (_, projects, _) = tally(&samples, true);
        let projects = projects.unwrap();
        assert_eq!(projects.samples, 3);
        assert_eq!(projects.counts[&Some("p1".to_string())], 1);
        assert_eq!(projects.counts[&None], 2);
    }
}
