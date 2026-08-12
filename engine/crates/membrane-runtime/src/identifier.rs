use std::path::Path;

pub const WORKTREE_DOC_PREFIX: &str = "doc://repo/worktree/";
pub const ANCHOR_PREFIX: &str = "mr://anchor/";

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum WorktreeDocRefError {
    #[error("worktree document reference must start with doc://repo/worktree/")]
    PrefixMismatch,
    #[error("worktree document reference names another repository or source kind")]
    CrossRepository,
    #[error("worktree document reference path is empty")]
    EmptyPath,
    #[error("worktree document reference contains traversal")]
    Traversal,
    #[error("worktree document reference contains an alias collision")]
    AliasCollision,
    #[error("worktree document reference contains an ambiguous Unicode character")]
    UnicodeAmbiguity,
    #[error("worktree document reference contains a Windows path form")]
    WindowsPath,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorktreeDocRef<'a> {
    relative: &'a str,
}

impl<'a> WorktreeDocRef<'a> {
    pub fn parse(value: &'a str) -> Result<Self, WorktreeDocRefError> {
        let relative = match value.strip_prefix(WORKTREE_DOC_PREFIX) {
            Some(relative) => relative,
            None if value.starts_with("doc://") => {
                return Err(WorktreeDocRefError::CrossRepository)
            }
            None => return Err(WorktreeDocRefError::PrefixMismatch),
        };
        validate_worktree_relative_path(relative)?;
        Ok(Self { relative })
    }

    pub fn relative_path(self) -> &'a str {
        self.relative
    }
}

fn validate_worktree_relative_path(relative: &str) -> Result<(), WorktreeDocRefError> {
    if relative.is_empty() {
        return Err(WorktreeDocRefError::EmptyPath);
    }
    if relative.contains('\\') || relative.contains(':') {
        return Err(WorktreeDocRefError::WindowsPath);
    }
    if relative.starts_with('/')
        || relative.ends_with('/')
        || relative.split('/').any(|part| matches!(part, "." | ".."))
        || Path::new(relative).is_absolute()
    {
        return Err(WorktreeDocRefError::Traversal);
    }
    if relative.contains("//") || contains_percent_escape(relative) {
        return Err(WorktreeDocRefError::AliasCollision);
    }
    if relative.chars().any(is_ambiguous_unicode) {
        return Err(WorktreeDocRefError::UnicodeAmbiguity);
    }
    if relative.chars().any(char::is_control) {
        return Err(WorktreeDocRefError::AliasCollision);
    }
    Ok(())
}

fn contains_percent_escape(value: &str) -> bool {
    value.as_bytes().windows(3).any(|bytes| {
        bytes[0] == b'%' && bytes[1].is_ascii_hexdigit() && bytes[2].is_ascii_hexdigit()
    })
}

fn is_ambiguous_unicode(value: char) -> bool {
    matches!(
        value,
        '\u{2044}' | '\u{2215}' | '\u{29f8}' | '\u{ff0f}' | '\u{ff3c}'
    ) || ('\u{202a}'..='\u{202e}').contains(&value)
        || ('\u{2066}'..='\u{2069}').contains(&value)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AnchorRefError {
    #[error("anchor reference must start with mr://anchor/")]
    PrefixMismatch,
    #[error("anchor reference names another namespace")]
    CrossNamespace,
    #[error("anchor digest must be 64 lowercase hexadecimal characters")]
    InvalidDigest,
    #[error("anchor digest has a non-canonical case alias")]
    AliasCollision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnchorRef<'a> {
    digest: &'a str,
}

impl<'a> AnchorRef<'a> {
    pub fn parse(value: &'a str) -> Result<Self, AnchorRefError> {
        let digest = match value.strip_prefix(ANCHOR_PREFIX) {
            Some(digest) => digest,
            None if value.starts_with("mr://") => return Err(AnchorRefError::CrossNamespace),
            None => return Err(AnchorRefError::PrefixMismatch),
        };
        if digest.len() != 64 {
            return Err(AnchorRefError::InvalidDigest);
        }
        if digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            && digest.bytes().any(|byte| byte.is_ascii_uppercase())
        {
            return Err(AnchorRefError::AliasCollision);
        }
        if !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(AnchorRefError::InvalidDigest);
        }
        Ok(Self { digest })
    }

    pub fn digest(self) -> &'a str {
        self.digest
    }
}
