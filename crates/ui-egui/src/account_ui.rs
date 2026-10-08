//! Discoverable account settings. Public backend state drives this window; the shell never
//! receives, persists or displays OAuth credentials.
use egui::RichText;
use serde_json::json;

use crate::{PhotocraftApp, menus, theme::Tokens, widgets};
use photocraft_engine::account_cmds::MANAGE_USAGE;

fn invoke(app: &mut PhotocraftApp, ctx: &egui::Context, id: &str, params: serde_json::Value) {
    if let Err(error) = menus::invoke(app, ctx, id, params) {
        app.ui.status = error;
        app.ui.status_error = true;
    }
}

pub fn settings_link(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let status = app.session.chatgpt_status();
    ui.add_space(10.0);
    ui.label(RichText::new("ChatGPT account").font(crate::theme::semibold(12.5)).color(t.text));
    if let Some(account) = status.accounts.iter().find(|a| status.active.as_ref() == Some(&a.id)) {
        ui.label(format!("Connected: {}", account.label));
    } else {
        ui.label(RichText::new("Connect through your system browser.").color(t.text_dim));
    }
    ui.label(RichText::new("Background removal and object selection run locally and do not use your ChatGPT plan.").color(t.text_dim));
    if widgets::secondary_button(ui, "ChatGPT account settings…", 210.0).clicked() {
        app.ui.chatgpt_account_open = true;
    }
}

pub fn show(app: &mut PhotocraftApp, ctx: &egui::Context) {
    let status = app.session.chatgpt_status();
    let active = status.accounts.iter().find(|a| status.active.as_ref() == Some(&a.id));
    let busy = app.session.jobs().iter().any(|j| j.command.starts_with("account.chatgpt."));
    let t = Tokens::get(ctx);
    let mut command = None;
    let mut open = app.ui.chatgpt_account_open;
    if open {
        egui::Window::new("ChatGPT account").id(egui::Id::new("chatgpt-account"))
            .open(&mut open).collapsible(false).resizable(false).default_width(400.0).show(ctx, |ui| {
                ui.label(RichText::new("Sign in with ChatGPT").font(crate::theme::semibold(16.0)).color(t.text));
                ui.label("Connect your ChatGPT account through OpenAI in your system browser.");
                ui.add_space(6.0);
                ui.label(RichText::new("This build connects your account but sends no AI requests. Generative image features are coming soon. Background removal and object selection use local algorithms or your downloaded models.").color(t.text_dim));
                ui.add_space(8.0);
                if let Some(a) = active {
                    ui.label(format!("Connected: {}", a.label));
                    if let Some(email) = &a.email { ui.label(RichText::new(email).color(t.text_dim)); }
                    ui.label(if a.plan_usage { "ChatGPT plan permission granted" } else { "Signed in for identity; ChatGPT plan use was not authorized" });
                    ui.add_enabled_ui(!busy, |ui| {
                        ui.horizontal(|ui| {
                            if widgets::secondary_button(ui, "Refresh connection", 150.0).clicked() { command = Some(("account.chatgpt.refresh", json!({}))); }
                            if widgets::secondary_button(ui, "Sign out", 100.0).clicked() { command = Some(("account.chatgpt.signOut", json!({}))); }
                        });
                    });
                }
                ui.add_enabled_ui(status.sign_in_available && !busy, |ui| {
                    let label = if active.is_some() { "Add another ChatGPT account" } else { "Continue with ChatGPT" };
                    if widgets::primary_button(ui, label, 240.0).clicked() { command = Some(("account.chatgpt.signIn", json!({}))); }
                    for a in &status.accounts {
                        ui.push_id(&a.id, |ui| {
                            if ui.selectable_label(status.active.as_ref() == Some(&a.id), format!("Continue as {}", a.label)).clicked() {
                                command = Some(("account.chatgpt.select", json!({"account":a.id})));
                            }
                        });
                    }
                });
                if !status.sign_in_available {
                    ui.label(RichText::new("ChatGPT sign-in is unavailable in this build.").color(t.text_faint));
                }
                if busy {
                    ui.label("Finish authorization in your browser. Progress and Cancel are in the status bar.");
                }
                if let Some(last) = app.session.jobs_with_recent().iter().rev().find(|j| j.command.starts_with("account.chatgpt."))
                    && let Some(result) = &last.result
                    && result.get("remoteRevocationConfirmed").and_then(|v| v.as_bool()) == Some(false)
                {
                    ui.label(RichText::new(result["message"].as_str().unwrap_or("Remote revocation was not confirmed; disconnect PhotoCraft in ChatGPT Settings.")).color(t.text));
                }
                ui.add_space(8.0);
                ui.hyperlink_to("Manage usage", MANAGE_USAGE);
                ui.hyperlink_to("About Sign in with ChatGPT", "https://developers.openai.com/siwc/token-sharing-open-source/sign-in");
            });
    }
    app.ui.chatgpt_account_open = open;
    if let Some(a) = active.filter(|a| a.plan_usage && !a.welcomed) {
        egui::Modal::new(egui::Id::new("chatgpt-plan-welcome")).show(ctx, |ui| {
            ui.set_max_width(400.0);
            ui.label(RichText::new("ChatGPT plan connected").font(crate::theme::semibold(16.0)).color(t.text));
            ui.label(
                "You authorized PhotoCraft to use your ChatGPT plan for eligible AI requests. This build makes no AI requests and consumes no plan usage.",
            );
            ui.hyperlink_to("Manage usage", MANAGE_USAGE);
            ui.add_enabled_ui(!busy, |ui| {
                if widgets::primary_button(ui, "Got it", 100.0).clicked() {
                    command = Some(("account.chatgpt.acknowledgeWelcome", json!({"account":a.id})));
                }
            });
        });
    }
    if let Some((id, params)) = command {
        invoke(app, ctx, id, params);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn account_settings_are_visible_without_a_document_and_honest_without_a_backend() {
        let mut h = Harness::builder().with_size(egui::vec2(1100.0, 800.0)).with_max_steps(64).build_eframe(|cc| {
            PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
            let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
            app.ui.chatgpt_account_open = true;
            app
        });
        h.run_steps(8);
        assert!(h.query_by_label("Continue with ChatGPT").is_some());
        assert!(h.query_by_label("ChatGPT sign-in is unavailable in this build.").is_some());
        assert!(h.query_by_label("Manage usage").is_some());
        assert!(h.query_by_label("Generate").is_none());
    }
}
