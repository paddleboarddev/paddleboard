// PaddleBoard: the persona system core. A persona is a markdown file describing
// who the agent should be — a Senior Developer, an SRE, a QA Engineer — which
// PaddleBoard injects as a session-stable system-prompt overlay. This crate is
// pure logic (parse, discover, assemble); selection UI lives in `agent_ui` and
// the AI Dock, and injection happens in the `agent` crate's system prompt.
//
// The design mirrors how SKILLS.md/CLAUDE.md conventions work: the model never
// "recognizes" a file — this harness reads it and puts it in context.

use anyhow::{Context as _, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// The zero-config entry point: a `PERSONA.md` at the project root is the
/// project's default persona, auto-adopted by new agent threads.
pub const ROOT_PERSONA_FILE: &str = "PERSONA.md";

/// Library personas live in `.claude/personas/*.persona.md`, project-scoped or
/// user-scoped (`~/.claude/personas/`), mirroring skills in `.claude/commands/`.
pub const PERSONA_FILE_SUFFIX: &str = ".persona.md";

const DEFAULT_ROOT_NAME: &str = "project-persona";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonaSource {
    /// `PERSONA.md` at the project root.
    ProjectRoot,
    /// `{project}/.claude/personas/*.persona.md`.
    ProjectLibrary,
    /// `~/.claude/personas/*.persona.md`.
    UserLibrary,
}

impl PersonaSource {
    pub fn label(&self) -> &'static str {
        match self {
            PersonaSource::ProjectRoot => "Project PERSONA.md",
            PersonaSource::ProjectLibrary => "Project",
            PersonaSource::UserLibrary => "User",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Persona {
    pub name: String,
    pub description: String,
    /// `role` (a job/function), `self` (models the user), or `person` (a
    /// specific individual). Free-form; purely descriptive.
    pub kind: String,
    /// A compact tone cue, folded into the overlay preamble.
    pub voice: String,
    pub body: String,
    /// Name of a persona this one inherits from (`extends:` frontmatter).
    /// The parent's body is prepended to the overlay; chains are followed
    /// (with cycle protection) so a role can build on a shared house base.
    pub extends: Option<String>,
    pub source: PersonaSource,
    pub path: PathBuf,
}

/// Parse a library persona file: flat `key: value` frontmatter between `---`
/// fences, then a markdown body. `name` and `description` are required — the
/// library index depends on them.
pub fn parse_persona(raw: &str, path: &Path, source: PersonaSource) -> Result<Persona> {
    let (front, body) = split_frontmatter(raw)
        .with_context(|| format!("{}: missing or malformed frontmatter", path.display()))?;
    let meta = parse_flat_frontmatter(front);

    let name = meta_value(&meta, "name")
        .with_context(|| format!("{}: frontmatter is missing 'name'", path.display()))?;
    let description = meta_value(&meta, "description")
        .with_context(|| format!("{}: frontmatter is missing 'description'", path.display()))?;

    Ok(Persona {
        name,
        description,
        kind: meta_value(&meta, "type").unwrap_or_else(|| "role".to_string()),
        voice: meta_value(&meta, "voice").unwrap_or_default(),
        body: body.trim().to_string(),
        extends: meta_value(&meta, "extends"),
        source,
        path: path.to_path_buf(),
    })
}

/// Parse a root `PERSONA.md` leniently: frontmatter is optional, and a plain
/// markdown description ("You are a senior SRE…") is a valid persona. This is
/// the zero-config front door, so it must not demand ceremony.
pub fn parse_root_persona(raw: &str, path: &Path) -> Option<Persona> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some((front, body)) = split_frontmatter(raw) {
        let meta = parse_flat_frontmatter(front);
        let body = body.trim();
        if body.is_empty() {
            return None;
        }
        return Some(Persona {
            name: meta_value(&meta, "name").unwrap_or_else(|| DEFAULT_ROOT_NAME.to_string()),
            description: meta_value(&meta, "description")
                .unwrap_or_else(|| first_prose_line(body)),
            kind: meta_value(&meta, "type").unwrap_or_else(|| "role".to_string()),
            voice: meta_value(&meta, "voice").unwrap_or_default(),
            body: body.to_string(),
            extends: meta_value(&meta, "extends"),
            source: PersonaSource::ProjectRoot,
            path: path.to_path_buf(),
        });
    }

    Some(Persona {
        name: DEFAULT_ROOT_NAME.to_string(),
        description: first_prose_line(trimmed),
        kind: "role".to_string(),
        voice: String::new(),
        body: trimmed.to_string(),
        extends: None,
        source: PersonaSource::ProjectRoot,
        path: path.to_path_buf(),
    })
}

/// Discover every persona visible from a project: the root `PERSONA.md` first,
/// then the project library, then the user library. A project-library persona
/// shadows a user-library persona with the same name.
pub fn discover(project_root: Option<&Path>) -> Vec<Persona> {
    let mut personas = Vec::new();

    if let Some(root) = project_root {
        let root_file = root.join(ROOT_PERSONA_FILE);
        if let Ok(raw) = fs::read_to_string(&root_file) {
            personas.extend(parse_root_persona(&raw, &root_file));
        }
        personas.extend(discover_library(
            &root.join(".claude").join("personas"),
            PersonaSource::ProjectLibrary,
        ));
    }

    personas.extend(discover_library(
        &paths::home_dir().join(".claude").join("personas"),
        PersonaSource::UserLibrary,
    ));

    let mut seen = Vec::new();
    personas.retain(|persona| {
        if seen.contains(&persona.name) {
            false
        } else {
            seen.push(persona.name.clone());
            true
        }
    });
    personas
}

fn discover_library(dir: &Path, source: PersonaSource) -> Vec<Persona> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut personas: Vec<Persona> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(PERSONA_FILE_SUFFIX))
        })
        .filter_map(|entry| {
            let path = entry.path();
            let raw = fs::read_to_string(&path).ok()?;
            match parse_persona(&raw, &path, source) {
                Ok(persona) => Some(persona),
                Err(error) => {
                    log::warn!("paddleboard_personas: skipping {}: {error:#}", path.display());
                    None
                }
            }
        })
        .collect();
    personas.sort_by(|a, b| a.name.cmp(&b.name));
    personas
}

/// The awareness preamble converts a file on disk into behavior: it tells the
/// model the text that follows is an identity to inhabit, scopes it to the
/// whole conversation, and draws the line that a persona changes style and
/// priorities, not honesty or capability.
///
/// `all_personas` supplies `extends:` parents: each ancestor's body is
/// included root-first under an "Inherited from" heading, so a role can build
/// on a shared house base. Cycles are cut and missing parents are logged and
/// skipped — the child persona always works on its own.
pub fn build_overlay(persona: &Persona, all_personas: &[Persona]) -> String {
    let voice = if persona.voice.is_empty() {
        "as described below".to_string()
    } else {
        persona.voice.clone()
    };

    let mut chain: Vec<&Persona> = Vec::new();
    let mut visited: Vec<&str> = vec![persona.name.as_str()];
    let mut next_parent = persona.extends.as_deref();
    while let Some(parent_name) = next_parent {
        if visited
            .iter()
            .any(|name| name.eq_ignore_ascii_case(parent_name))
        {
            log::warn!(
                "paddleboard_personas: `extends` cycle at '{parent_name}' (from '{}'); stopping",
                persona.name
            );
            break;
        }
        let Some(parent) = all_personas
            .iter()
            .find(|candidate| candidate.name.eq_ignore_ascii_case(parent_name))
        else {
            log::warn!(
                "paddleboard_personas: '{}' extends unknown persona '{parent_name}'; skipping",
                persona.name
            );
            break;
        };
        visited.push(parent.name.as_str());
        chain.push(parent);
        next_parent = parent.extends.as_deref();
    }

    let mut inherited = String::new();
    // Root-most ancestor first, so the child's own body always reads last
    // and wins where they conflict.
    for parent in chain.iter().rev() {
        inherited.push_str(&format!(
            "## Inherited from `{}`\n\n{}\n\n",
            parent.name, parent.body
        ));
    }

    format!(
        "You are operating under a PERSONA. Adopt the identity, values, voice, and \
         behavioral rules defined below and hold them for the entire conversation \
         until the user switches or clears the persona. The persona shapes *how* \
         you respond — your tone, priorities, and what you push back on — not your \
         underlying capabilities or honesty.\n\n\
         Active persona: {} ({}). Voice: {}.\n\n---\n\n{}{}",
        persona.name, persona.kind, voice, inherited, persona.body
    )
}

fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
    let rest = raw.strip_prefix("---")?;
    let rest = rest.strip_prefix("\r\n").or_else(|| rest.strip_prefix('\n'))?;
    let end = rest.find("\n---").map(|ix| (ix, ix + 4)).or_else(|| {
        rest.find("\r\n---").map(|ix| (ix, ix + 5))
    })?;
    let front = &rest[..end.0];
    let body = rest[end.1..].trim_start_matches(['\r', '\n']);
    Some((front, body))
}

fn parse_flat_frontmatter(front: &str) -> Vec<(String, String)> {
    front
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            let value = value.trim().trim_matches(['"', '\'']);
            Some((key.trim().to_string(), value.to_string()))
        })
        .collect()
}

fn meta_value(meta: &[(String, String)], key: &str) -> Option<String> {
    meta.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty())
}

fn first_prose_line(body: &str) -> String {
    let line = body
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .unwrap_or("Project persona from PERSONA.md");
    let mut description: String = line.chars().take(120).collect();
    if line.chars().count() > 120 {
        description.push('…');
    }
    description
}

#[cfg(test)]
mod tests {
    use super::*;

    const QA: &str = "---\nname: qa-tester\ndescription: A meticulous QA engineer.\ntype: role\nvoice: terse, skeptical\n---\n\n# Identity\n\nYou think in failure modes.\n";

    #[test]
    fn parses_library_persona() {
        let persona =
            parse_persona(QA, Path::new("qa.persona.md"), PersonaSource::UserLibrary).unwrap();
        assert_eq!(persona.name, "qa-tester");
        assert_eq!(persona.description, "A meticulous QA engineer.");
        assert_eq!(persona.kind, "role");
        assert_eq!(persona.voice, "terse, skeptical");
        assert!(persona.body.starts_with("# Identity"));
    }

    #[test]
    fn library_persona_requires_name_and_description() {
        let raw = "---\ndescription: no name here\n---\nbody";
        assert!(parse_persona(raw, Path::new("x.persona.md"), PersonaSource::UserLibrary).is_err());
        let raw = "---\nname: no-description\n---\nbody";
        assert!(parse_persona(raw, Path::new("x.persona.md"), PersonaSource::UserLibrary).is_err());
    }

    #[test]
    fn root_persona_accepts_plain_markdown() {
        let persona =
            parse_root_persona("You are a senior SRE.\nAlways ask about rollback.", Path::new("PERSONA.md"))
                .unwrap();
        assert_eq!(persona.name, "project-persona");
        assert_eq!(persona.description, "You are a senior SRE.");
        assert_eq!(persona.source, PersonaSource::ProjectRoot);
    }

    #[test]
    fn root_persona_honors_optional_frontmatter() {
        let raw = "---\nname: site-reliability\nvoice: calm under fire\n---\n\n# Identity\nSRE.";
        let persona = parse_root_persona(raw, Path::new("PERSONA.md")).unwrap();
        assert_eq!(persona.name, "site-reliability");
        assert_eq!(persona.voice, "calm under fire");
        assert_eq!(persona.description, "SRE.");
    }

    #[test]
    fn root_persona_empty_is_none() {
        assert!(parse_root_persona("   \n", Path::new("PERSONA.md")).is_none());
    }

    #[test]
    fn overlay_contains_preamble_and_body() {
        let persona =
            parse_persona(QA, Path::new("qa.persona.md"), PersonaSource::UserLibrary).unwrap();
        let overlay = build_overlay(&persona, &[]);
        assert!(overlay.contains("operating under a PERSONA"));
        assert!(overlay.contains("Active persona: qa-tester (role). Voice: terse, skeptical."));
        assert!(overlay.contains("You think in failure modes."));
    }

    const HOUSE_BASE: &str = "---\nname: house-base\ndescription: Shared house rules.\n---\n\n- Always cite file paths.\n";
    const EXTENDED_QA: &str = "---\nname: strict-qa\ndescription: QA on the house base.\nextends: house-base\n---\n\n- Demand repro steps.\n";

    #[test]
    fn overlay_includes_extends_chain_root_first() {
        let base =
            parse_persona(HOUSE_BASE, Path::new("base.persona.md"), PersonaSource::UserLibrary)
                .unwrap();
        let child =
            parse_persona(EXTENDED_QA, Path::new("qa.persona.md"), PersonaSource::UserLibrary)
                .unwrap();
        assert_eq!(child.extends.as_deref(), Some("house-base"));

        let all = vec![base, child.clone()];
        let overlay = build_overlay(&child, &all);
        let inherited_ix = overlay.find("Inherited from `house-base`").unwrap();
        let base_rule_ix = overlay.find("Always cite file paths.").unwrap();
        let child_rule_ix = overlay.find("Demand repro steps.").unwrap();
        assert!(inherited_ix < base_rule_ix && base_rule_ix < child_rule_ix);
    }

    #[test]
    fn overlay_survives_extends_cycle_and_missing_parent() {
        let cyclic_a = "---\nname: a\ndescription: a.\nextends: b\n---\nbody a";
        let cyclic_b = "---\nname: b\ndescription: b.\nextends: a\n---\nbody b";
        let a = parse_persona(cyclic_a, Path::new("a.persona.md"), PersonaSource::UserLibrary)
            .unwrap();
        let b = parse_persona(cyclic_b, Path::new("b.persona.md"), PersonaSource::UserLibrary)
            .unwrap();
        let all = vec![a.clone(), b];
        let overlay = build_overlay(&a, &all);
        assert!(overlay.contains("body a"));
        assert!(overlay.contains("Inherited from `b`"));
        // The cycle back to `a` is cut — `a` appears as the active persona only.
        assert_eq!(overlay.matches("Inherited from").count(), 1);

        let orphan = "---\nname: orphan\ndescription: o.\nextends: nowhere\n---\nbody o";
        let orphan =
            parse_persona(orphan, Path::new("o.persona.md"), PersonaSource::UserLibrary).unwrap();
        let overlay = build_overlay(&orphan, std::slice::from_ref(&orphan));
        assert!(overlay.contains("body o"));
        assert!(!overlay.contains("Inherited from"));
    }

    #[test]
    fn discovery_orders_and_shadows() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("PERSONA.md"), "You are a pirate captain.").unwrap();
        let lib = root.join(".claude").join("personas");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("qa.persona.md"), QA).unwrap();
        std::fs::write(lib.join("bad.persona.md"), "no frontmatter at all").unwrap();

        let personas = discover(Some(root));
        // Root persona first, unparseable library file skipped.
        assert_eq!(personas[0].name, "project-persona");
        assert!(personas.iter().any(|p| p.name == "qa-tester"));
        assert!(!personas.iter().any(|p| p.path.ends_with("bad.persona.md")));
    }
}
