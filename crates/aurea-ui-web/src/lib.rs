use std::collections::HashMap;
use std::sync::OnceLock;

use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const I18N_JSON: &str = include_str!("../../../ux/i18n_keys.json");
const STYLE: &str = r#"
:root {
  --aurea-bg: radial-gradient(circle at top right, #f4eee1, #f8f5ef 45%, #ece7da 100%);
  --aurea-panel: #fffdf8;
  --aurea-ink: #18202a;
  --aurea-muted: #5f6c79;
  --aurea-accent: #0d6f7a;
  --aurea-accent-2: #0f9e85;
  --aurea-border: #d3c8b7;
  --aurea-local: #1f8f47;
  --aurea-danger: #b0362e;
}

.aurea-shell {
  background: var(--aurea-bg);
  color: var(--aurea-ink);
  border: 1px solid var(--aurea-border);
  border-radius: 18px;
  max-width: 720px;
  margin: 1rem auto;
  padding: 1rem;
  font-family: "Space Grotesk", "Avenir Next", "Segoe UI", sans-serif;
  box-shadow: 0 14px 34px rgba(14, 36, 44, 0.12);
}

.aurea-topline {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 0.5rem;
}

.aurea-pill {
  display: inline-flex;
  align-items: center;
  border-radius: 999px;
  padding: 0.2rem 0.65rem;
  font-size: 0.78rem;
  font-weight: 700;
  letter-spacing: 0.02em;
  border: 1px solid transparent;
}

.aurea-pill-local {
  color: var(--aurea-local);
  border-color: color-mix(in srgb, var(--aurea-local), white 60%);
  background: color-mix(in srgb, var(--aurea-local), white 88%);
}

.aurea-pill-sign-ok {
  color: var(--aurea-accent);
  border-color: color-mix(in srgb, var(--aurea-accent), white 55%);
  background: color-mix(in srgb, var(--aurea-accent), white 90%);
}

.aurea-pill-sign-fail {
  color: var(--aurea-danger);
  border-color: color-mix(in srgb, var(--aurea-danger), white 55%);
  background: color-mix(in srgb, var(--aurea-danger), white 90%);
}

.aurea-code {
  display: block;
  font-family: "IBM Plex Mono", "Menlo", monospace;
  font-size: 0.78rem;
  color: var(--aurea-muted);
  overflow-wrap: anywhere;
}

.aurea-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: 0.75rem;
  margin: 0.8rem 0;
}

.aurea-panel {
  background: var(--aurea-panel);
  border: 1px solid color-mix(in srgb, var(--aurea-border), white 20%);
  border-radius: 12px;
  padding: 0.8rem;
}

.aurea-panel h3 {
  margin: 0;
  font-size: 0.82rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--aurea-muted);
}

.aurea-panel p,
.aurea-panel ul {
  margin: 0.45rem 0 0;
}

.aurea-actions {
  display: grid;
  gap: 0.45rem;
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.aurea-btn {
  border: 1px solid color-mix(in srgb, var(--aurea-accent), white 52%);
  border-radius: 10px;
  padding: 0.55rem 0.65rem;
  font-weight: 700;
  font-size: 0.9rem;
  background: color-mix(in srgb, var(--aurea-accent), white 88%);
  color: #0f3f4c;
  cursor: pointer;
}

.aurea-btn[data-strong="true"] {
  background: linear-gradient(92deg, var(--aurea-accent), var(--aurea-accent-2));
  color: #fff;
  border-color: color-mix(in srgb, var(--aurea-accent), black 8%);
}

.aurea-btn:focus-visible {
  outline: 3px solid color-mix(in srgb, var(--aurea-accent), white 25%);
  outline-offset: 2px;
}

.aurea-list {
  margin: 0;
  padding-left: 1.1rem;
}

.aurea-list li {
  margin: 0.2rem 0;
}

@media (max-width: 520px) {
  .aurea-shell {
    border-radius: 14px;
    padding: 0.85rem;
  }

  .aurea-actions {
    grid-template-columns: 1fr;
  }
}
"#;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Pt,
    En,
}

impl Lang {
    pub fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("pt").to_ascii_lowercase().as_str() {
            "en" => Self::En,
            _ => Self::Pt,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanCardData {
    pub plan_hash: String,
    pub dag_summary: String,
    pub slos: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub costs: Option<String>,
    pub policy_trace: Vec<String>,
    pub local_only: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptArtifactView {
    pub cid: String,
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptData {
    pub cid: String,
    pub signature_ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_href: Option<String>,
    #[serde(default)]
    pub artifacts: Vec<ReceiptArtifactView>,
}

#[derive(Debug, Error)]
pub enum UiError {
    #[error("failed to parse i18n bundle: {0}")]
    I18nParse(serde_json::Error),
}

#[derive(Debug, Clone, Deserialize)]
struct I18nBundle {
    pt: LocalePack,
    en: LocalePack,
}

#[derive(Debug, Clone, Deserialize)]
struct LocalePack {
    buttons: Buttons,
    labels: Labels,
    errors: HashMap<String, String>,
    tooltips: Tooltips,
}

#[derive(Debug, Clone, Deserialize)]
struct Buttons {
    confirm: String,
    edit: String,
    schedule: String,
    verify: String,
    repeat: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Labels {
    plan: String,
    receipt: String,
    policy_trace: String,
    local_only: String,
    costs: String,
    slos: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Tooltips {
    policy_trace: String,
    local_only: String,
}

fn i18n_bundle() -> Result<&'static I18nBundle, UiError> {
    static BUNDLE: OnceLock<I18nBundle> = OnceLock::new();
    if let Some(bundle) = BUNDLE.get() {
        return Ok(bundle);
    }

    let parsed = serde_json::from_str(I18N_JSON).map_err(UiError::I18nParse)?;
    Ok(BUNDLE.get_or_init(|| parsed))
}

fn locale(bundle: &I18nBundle, lang: Lang) -> &LocalePack {
    match lang {
        Lang::Pt => &bundle.pt,
        Lang::En => &bundle.en,
    }
}

pub fn error_message(code: &str, lang: Lang) -> Result<String, UiError> {
    let bundle = i18n_bundle()?;
    let locale = locale(bundle, lang);
    Ok(locale
        .errors
        .get(code)
        .cloned()
        .unwrap_or_else(|| code.to_string()))
}

pub fn render_plan_card_html(plan: &PlanCardData, lang: Lang) -> Result<String, UiError> {
    let bundle = i18n_bundle()?;
    let l = locale(bundle, lang).clone();

    let plan_hash = plan.plan_hash.clone();
    let dag_summary = plan.dag_summary.clone();
    let slos = plan.slos.clone();
    let costs = plan.costs.clone().unwrap_or_else(|| "-".to_string());
    let policy_trace = if plan.policy_trace.is_empty() {
        vec!["baseline_accept".to_string()]
    } else {
        plan.policy_trace.clone()
    };
    let local_only = plan.local_only;
    let warnings = plan.warnings.clone();

    let html = Owner::new().with(|| {
        view! {
            <section class="aurea-shell" role="region" aria-label={l.labels.plan.clone()}>
                <style>{STYLE}</style>
                <div class="aurea-topline">
                    <h2>{l.labels.plan.clone()}</h2>
                    {if local_only {
                        view! {
                            <span class="aurea-pill aurea-pill-local" role="status" title={l.tooltips.local_only.clone()}>
                                {l.labels.local_only.clone()}
                            </span>
                        }.into_any()
                    } else {
                        view! { <span class="aurea-code">""</span> }.into_any()
                    }}
                </div>

                <span class="aurea-code">{format!("plan_hash={} ", plan_hash)}</span>

                <div class="aurea-grid">
                    <article class="aurea-panel">
                        <h3>"DAG"</h3>
                        <p>{dag_summary}</p>
                    </article>
                    <article class="aurea-panel">
                        <h3>{l.labels.slos.clone()}</h3>
                        <p>{slos}</p>
                    </article>
                    <article class="aurea-panel">
                        <h3>{l.labels.costs.clone()}</h3>
                        <p>{costs}</p>
                    </article>
                </div>

                <article class="aurea-panel" aria-label={l.labels.policy_trace.clone()} title={l.tooltips.policy_trace.clone()}>
                    <h3>{l.labels.policy_trace.clone()}</h3>
                    <ul class="aurea-list">
                        {policy_trace
                            .into_iter()
                            .map(|entry| view! { <li>{entry}</li> })
                            .collect_view()}
                    </ul>
                </article>

                {if warnings.is_empty() {
                    ().into_any()
                } else {
                    view! {
                        <article class="aurea-panel" aria-label="warnings">
                            <h3>"Warnings"</h3>
                            <ul class="aurea-list">
                                {warnings
                                    .into_iter()
                                    .map(|entry| view! { <li>{entry}</li> })
                                    .collect_view()}
                            </ul>
                        </article>
                    }.into_any()
                }}

                <div class="aurea-actions" role="group" aria-label="plan actions">
                    <button class="aurea-btn" data-strong="true" aria-label={l.buttons.confirm.clone()}>{l.buttons.confirm.clone()}</button>
                    <button class="aurea-btn" aria-label={l.buttons.edit.clone()}>{l.buttons.edit.clone()}</button>
                    <button class="aurea-btn" aria-label={l.buttons.schedule.clone()}>{l.buttons.schedule.clone()}</button>
                    <button class="aurea-btn" aria-label={l.buttons.repeat.clone()}>{l.buttons.repeat.clone()}</button>
                </div>
            </section>
        }
        .to_html()
    });

    Ok(html)
}

pub fn render_receipt_html(receipt: &ReceiptData, lang: Lang) -> Result<String, UiError> {
    let bundle = i18n_bundle()?;
    let l = locale(bundle, lang).clone();

    let cid = receipt.cid.clone();
    let signature_ok = receipt.signature_ok;
    let anchor_href = receipt.anchor_href.clone();
    let artifacts = receipt.artifacts.clone();

    let html = Owner::new().with(|| {
        view! {
            <section class="aurea-shell" role="region" aria-label={l.labels.receipt.clone()}>
                <style>{STYLE}</style>
                <div class="aurea-topline">
                    <h2>{l.labels.receipt.clone()}</h2>
                    {if signature_ok {
                        view! { <span class="aurea-pill aurea-pill-sign-ok" role="status">"signature: ok"</span> }.into_any()
                    } else {
                        view! { <span class="aurea-pill aurea-pill-sign-fail" role="status">"signature: fail"</span> }.into_any()
                    }}
                </div>
                <span class="aurea-code">{format!("cid={} ", cid)}</span>

                <div class="aurea-grid">
                    <article class="aurea-panel">
                        <h3>"Anchor"</h3>
                        {if let Some(anchor_href) = anchor_href {
                            view! { <a href={anchor_href}>"anchor"</a> }.into_any()
                        } else {
                            view! { <p>"-"</p> }.into_any()
                        }}
                    </article>
                    <article class="aurea-panel">
                        <h3>"Artifacts"</h3>
                        <ul class="aurea-list">
                            {artifacts
                                .into_iter()
                                .map(|artifact| {
                                    let label = format!("{} ({} bytes)", artifact.path, artifact.size_bytes);
                                    view! {
                                        <li>
                                            <span>{label}</span>
                                            <span class="aurea-code">{format!("cid={} ", artifact.cid)}</span>
                                        </li>
                                    }
                                })
                                .collect_view()}
                        </ul>
                    </article>
                </div>

                <div class="aurea-actions" role="group" aria-label="receipt actions">
                    <button class="aurea-btn" data-strong="true" aria-label={l.buttons.verify.clone()}>{l.buttons.verify.clone()}</button>
                    <button class="aurea-btn" aria-label={l.buttons.repeat.clone()}>{l.buttons.repeat.clone()}</button>
                </div>
            </section>
        }
        .to_html()
    });

    Ok(html)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_map_supports_pt_and_en() {
        let pt = error_message("SCHEMA_INVALID", Lang::Pt).expect("pt message");
        let en = error_message("SCHEMA_INVALID", Lang::En).expect("en message");
        assert!(pt.contains("Faltam"));
        assert!(en.contains("Missing"));
    }

    #[test]
    fn plan_card_renders_local_only_badge() {
        let html = render_plan_card_html(
            &PlanCardData {
                plan_hash: "abc".to_string(),
                dag_summary: "vcx -> verify".to_string(),
                slos: "TTFT p95 <= 4s; TTR p95 <= 9s".to_string(),
                costs: None,
                policy_trace: vec!["pii_local=true".to_string()],
                local_only: true,
                warnings: vec![],
            },
            Lang::Pt,
        )
        .expect("render plan card");

        assert!(html.contains("Executa local"));
        assert!(html.contains("Confirmar"));
    }

    #[test]
    fn receipt_renders_artifacts() {
        let html = render_receipt_html(
            &ReceiptData {
                cid: "rcpt-1".to_string(),
                signature_ok: true,
                anchor_href: Some("/v1/anchors/2026-02-19".to_string()),
                artifacts: vec![ReceiptArtifactView {
                    cid: "cid-1".to_string(),
                    path: "./packs/a.vcxpack".to_string(),
                    size_bytes: 77,
                }],
            },
            Lang::En,
        )
        .expect("render receipt");

        assert!(html.contains("signature: ok"));
        assert!(html.contains("./packs/a.vcxpack"));
        assert!(html.contains("cid=cid-1"));
    }
}
