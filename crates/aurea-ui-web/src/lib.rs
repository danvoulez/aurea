use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanCard {
    pub dag_summary: String,
    pub slos: String,
    pub policy_trace: Vec<String>,
    pub local_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptView {
    pub cid: String,
    pub signature_ok: bool,
    pub anchor_ref: Option<String>,
}

pub fn render_plan_card(plan: &PlanCard, lang: &str) -> String {
    let confirm = if lang == "pt" { "Confirmar" } else { "Confirm" };
    format!(
        "PlanCard [{}] {} | SLOs: {} | policy={} | local_only={} | action={} ",
        lang,
        plan.dag_summary,
        plan.slos,
        plan.policy_trace.join(","),
        plan.local_only,
        confirm,
    )
}

pub fn render_receipt(view: &ReceiptView, lang: &str) -> String {
    let label = if lang == "pt" { "Recibo" } else { "Receipt" };
    format!(
        "{} {} signature_ok={} anchor={}",
        label,
        view.cid,
        view.signature_ok,
        view.anchor_ref.clone().unwrap_or_else(|| "n/a".to_string()),
    )
}
