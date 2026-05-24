use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RedactionFinding {
    pub kind: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub replacement: String,
    pub confidence: f64,
    pub blocks_sync: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PiiCoverage {
    pub status: PiiCoverageStatus,
    pub detector: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PiiCoverageStatus {
    Available,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RedactionOutcome {
    pub findings: Vec<RedactionFinding>,
    pub redacted_text: String,
    pub blocks_sync: bool,
    pub pii_coverage: PiiCoverage,
}

pub trait PrivacyDetector {
    fn detect(&self, text: &str) -> Vec<RedactionFinding>;
    fn coverage(&self) -> PiiCoverage;
}

#[derive(Debug, Default)]
pub struct UnavailablePrivacyDetector;

impl PrivacyDetector for UnavailablePrivacyDetector {
    fn detect(&self, _text: &str) -> Vec<RedactionFinding> {
        Vec::new()
    }

    fn coverage(&self) -> PiiCoverage {
        PiiCoverage {
            status: PiiCoverageStatus::Degraded,
            detector: "openai/privacy-filter".to_string(),
            reason: "optional privacy-filter detector is not configured; deterministic secret and coarse PII rules applied".to_string(),
        }
    }
}

#[derive(Debug, Default)]
pub struct DeterministicPrivacyDetector;

impl PrivacyDetector for DeterministicPrivacyDetector {
    fn detect(&self, text: &str) -> Vec<RedactionFinding> {
        detect_coarse_pii(text)
    }

    fn coverage(&self) -> PiiCoverage {
        PiiCoverage {
            status: PiiCoverageStatus::Available,
            detector: "deterministic-pii-rules".to_string(),
            reason: "local deterministic PII detector enabled".to_string(),
        }
    }
}

pub fn scan_text(text: &str) -> RedactionOutcome {
    scan_text_with_detector(text, &UnavailablePrivacyDetector)
}

pub fn scan_text_with_detector(text: &str, detector: &dyn PrivacyDetector) -> RedactionOutcome {
    let mut findings = detect_deterministic_secrets(text);
    findings.extend(detect_coarse_pii(text));
    findings.extend(detector.detect(text));
    let findings = merge_findings(findings);
    let redacted_text = apply_redactions(text, &findings);
    let blocks_sync = findings.iter().any(|finding| finding.blocks_sync);

    RedactionOutcome {
        findings,
        redacted_text,
        blocks_sync,
        pii_coverage: detector.coverage(),
    }
}

pub fn apply_redactions(text: &str, findings: &[RedactionFinding]) -> String {
    let mut redacted = String::with_capacity(text.len());
    let mut cursor = 0;
    for finding in findings {
        if finding.start_byte < cursor || finding.end_byte > text.len() {
            continue;
        }
        redacted.push_str(&text[cursor..finding.start_byte]);
        redacted.push_str(&finding.replacement);
        cursor = finding.end_byte;
    }
    redacted.push_str(&text[cursor..]);
    redacted
}

pub fn merge_findings(mut findings: Vec<RedactionFinding>) -> Vec<RedactionFinding> {
    findings.retain(|finding| {
        finding.start_byte < finding.end_byte && finding.start_byte <= finding.end_byte
    });
    findings.sort_by(|left, right| {
        left.start_byte
            .cmp(&right.start_byte)
            .then_with(|| right.end_byte.cmp(&left.end_byte))
            .then_with(|| right.blocks_sync.cmp(&left.blocks_sync))
    });

    let mut merged: Vec<RedactionFinding> = Vec::new();
    for finding in findings {
        let Some(current) = merged.last_mut() else {
            merged.push(finding);
            continue;
        };

        if finding.start_byte >= current.end_byte {
            merged.push(finding);
            continue;
        }

        current.end_byte = current.end_byte.max(finding.end_byte);
        current.blocks_sync |= finding.blocks_sync;
        current.confidence = current.confidence.max(finding.confidence);
        if current.kind != finding.kind {
            current.kind = if current.blocks_sync {
                "secret".to_string()
            } else {
                "pii".to_string()
            };
        }
        current.replacement = replacement_for(current.blocks_sync, &current.kind);
    }

    merged
}

fn detect_deterministic_secrets(text: &str) -> Vec<RedactionFinding> {
    let mut findings = Vec::new();

    for captures in key_value_secret_regex().captures_iter(text) {
        if let Some(secret) = captures.name("value") {
            findings.push(secret_finding(
                "key_value_secret",
                secret.start(),
                secret.end(),
                1.0,
            ));
        }
    }

    for pattern in [
        ("openai_api_key", openai_key_regex()),
        ("github_token", github_token_regex()),
        ("aws_access_key", aws_access_key_regex()),
        ("slack_token", slack_token_regex()),
        ("google_api_key", google_api_key_regex()),
        ("private_key", private_key_regex()),
    ] {
        for matched in pattern.1.find_iter(text) {
            findings.push(secret_finding(
                pattern.0,
                matched.start(),
                matched.end(),
                0.98,
            ));
        }
    }

    findings.extend(detect_high_entropy_tokens(text));
    findings
}

fn detect_coarse_pii(text: &str) -> Vec<RedactionFinding> {
    let mut findings = Vec::new();
    for matched in email_regex().find_iter(text) {
        findings.push(pii_finding(
            "private_email",
            matched.start(),
            matched.end(),
            0.95,
        ));
    }
    for matched in phone_regex().find_iter(text) {
        findings.push(pii_finding(
            "private_phone",
            matched.start(),
            matched.end(),
            0.85,
        ));
    }
    for matched in address_regex().find_iter(text) {
        findings.push(pii_finding(
            "private_address",
            matched.start(),
            matched.end(),
            0.75,
        ));
    }
    findings
}

fn detect_high_entropy_tokens(text: &str) -> Vec<RedactionFinding> {
    let mut findings = Vec::new();
    for matched in high_entropy_candidate_regex().find_iter(text) {
        let value = matched.as_str();
        if has_letters_and_digits(value) && shannon_entropy(value) >= 3.5 {
            findings.push(secret_finding(
                "high_entropy_secret",
                matched.start(),
                matched.end(),
                0.72,
            ));
        }
    }
    findings
}

fn secret_finding(
    kind: &str,
    start_byte: usize,
    end_byte: usize,
    confidence: f64,
) -> RedactionFinding {
    RedactionFinding {
        kind: kind.to_string(),
        start_byte,
        end_byte,
        replacement: replacement_for(true, kind),
        confidence,
        blocks_sync: true,
    }
}

fn pii_finding(
    kind: &str,
    start_byte: usize,
    end_byte: usize,
    confidence: f64,
) -> RedactionFinding {
    RedactionFinding {
        kind: kind.to_string(),
        start_byte,
        end_byte,
        replacement: replacement_for(false, kind),
        confidence,
        blocks_sync: false,
    }
}

fn replacement_for(blocks_sync: bool, kind: &str) -> String {
    if blocks_sync {
        "[REDACTED:secret]".to_string()
    } else {
        format!("[REDACTED:{kind}]")
    }
}

fn has_letters_and_digits(value: &str) -> bool {
    value.bytes().any(|byte| byte.is_ascii_alphabetic())
        && value.bytes().any(|byte| byte.is_ascii_digit())
}

fn shannon_entropy(value: &str) -> f64 {
    let mut alphabet = HashSet::new();
    let mut counts = [0usize; 256];
    for byte in value.bytes() {
        alphabet.insert(byte);
        counts[byte as usize] += 1;
    }
    if alphabet.len() < 8 {
        return 0.0;
    }

    let len = value.len() as f64;
    counts
        .iter()
        .filter(|count| **count > 0)
        .map(|count| {
            let p = *count as f64 / len;
            -p * p.log2()
        })
        .sum()
}

fn key_value_secret_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"(?im)["']?(?P<key>\b[A-Z0-9_.-]*(?:api[_-]?key|secret|token|password|passwd|pwd|private[_-]?key|access[_-]?key|credential)[A-Z0-9_.-]*\b)["']?\s*(?:=|:)\s*["']?(?P<value>[^\s"',}#]+)["']?"#,
        )
        .expect("key value secret regex")
    })
}

fn openai_key_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"\bsk-[A-Za-z0-9_-]{20,}\b").expect("openai key regex"))
}

fn github_token_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9_]{20,}\b|\bgithub_pat_[A-Za-z0-9_]{20,}\b",
        )
        .expect("github token regex")
    })
}

fn aws_access_key_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"\bAKIA[0-9A-Z]{16}\b").expect("aws access key regex"))
}

fn slack_token_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX
        .get_or_init(|| Regex::new(r"\bxox[baprs]-[A-Za-z0-9-]{20,}\b").expect("slack token regex"))
}

fn google_api_key_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"\bAIza[0-9A-Za-z_-]{30,}\b").expect("google api key regex"))
}

fn private_key_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----")
            .expect("private key regex")
    })
}

fn high_entropy_candidate_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"\b[A-Za-z0-9_+/=-]{32,}\b").expect("entropy regex"))
}

fn email_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b").expect("email regex")
    })
}

fn phone_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"\b(?:\+?1[-.\s]?)?\(?[2-9][0-9]{2}\)?[-.\s][0-9]{3}[-.\s][0-9]{4}\b")
            .expect("phone regex")
    })
}

fn address_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"\b\d{1,6}\s+[A-Z][A-Za-z0-9.-]*(?:\s+[A-Z][A-Za-z0-9.-]*){0,4}\s+(?:Street|St\.?|Avenue|Ave\.?|Road|Rd\.?|Lane|Ln\.?|Drive|Dr\.?)\b",
        )
        .expect("address regex")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_secret_findings_block_sync_and_redact_value() {
        let outcome =
            scan_text("OPENAI_API_KEY=sk-test-000000000000000000000000000000000000000000000000");

        assert!(outcome.blocks_sync);
        assert!(
            outcome
                .findings
                .iter()
                .any(|finding| finding.kind == "secret" || finding.kind == "key_value_secret")
        );
        assert!(!outcome.redacted_text.contains("sk-test"));
        assert!(outcome.redacted_text.contains("[REDACTED:secret]"));
    }

    #[test]
    fn pii_findings_redact_without_blocking_sync() {
        let outcome = scan_text("Contact ada@example.test or +1-202-555-0100.");

        assert!(!outcome.blocks_sync);
        assert!(outcome.redacted_text.contains("[REDACTED:private_email]"));
        assert!(outcome.redacted_text.contains("[REDACTED:private_phone]"));
        assert_eq!(outcome.pii_coverage.status, PiiCoverageStatus::Degraded);
    }

    #[test]
    fn overlapping_findings_are_merged_to_the_strongest_boundary() {
        let findings = merge_findings(vec![
            RedactionFinding {
                kind: "private_email".to_string(),
                start_byte: 4,
                end_byte: 14,
                replacement: "[REDACTED:private_email]".to_string(),
                confidence: 0.7,
                blocks_sync: false,
            },
            RedactionFinding {
                kind: "env_secret".to_string(),
                start_byte: 9,
                end_byte: 20,
                replacement: "[REDACTED:secret]".to_string(),
                confidence: 1.0,
                blocks_sync: true,
            },
        ]);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].start_byte, 4);
        assert_eq!(findings[0].end_byte, 20);
        assert!(findings[0].blocks_sync);
        assert_eq!(findings[0].replacement, "[REDACTED:secret]");
    }

    #[test]
    fn lowercase_and_structured_credential_values_block_sync() {
        for text in [
            "password=hunter2",
            r#"{"db_password":"hunter2"}"#,
            "api_key: short-secret",
            "token = abc123",
        ] {
            let outcome = scan_text(text);
            assert!(outcome.blocks_sync, "{text} should block sync");
            assert!(
                !outcome.redacted_text.contains("hunter2")
                    && !outcome.redacted_text.contains("short-secret")
                    && !outcome.redacted_text.contains("abc123"),
                "{text} should redact secret values"
            );
        }
    }
}
