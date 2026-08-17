use super::model::{Diagnostic, Report};

fn format_diagnostic(diagnostic: &Diagnostic) -> String {
    format!(
        "[{}] {} ({}): {}",
        diagnostic.status.label(),
        diagnostic.area.label(),
        diagnostic.code,
        diagnostic.message
    )
}

impl Report {
    /// Renders stable line-oriented human output.
    #[must_use]
    pub fn human(&self) -> String {
        let mut lines = Vec::with_capacity(self.diagnostics().len() + 1);
        lines.push(format!("doctor: {}", self.status().label()));
        lines.extend(self.diagnostics().iter().map(format_diagnostic));
        lines.join("\n")
    }

    /// Renders structured JSON without performing any I/O.
    ///
    /// # Errors
    /// Propagates JSON serialization failures.
    pub fn json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}
