#[derive(Clone, Debug, Default)]
pub struct DownloadProgress {
    pub percent: Option<f32>,
    pub speed: Option<String>,
    pub eta: Option<String>,
    pub current_file: Option<String>,
    pub phase: ProgressPhase,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ProgressPhase {
    #[default]
    Idle,
    Preparing,
    Downloading,
    PostProcessing,
    Finished,
    Canceled,
    Failed,
}

impl DownloadProgress {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn update_from_chunk(&mut self, chunk: &str) {
        for line in chunk.lines() {
            self.update_from_line(line.trim());
        }
    }

    pub fn label(&self) -> &'static str {
        match self.phase {
            ProgressPhase::Idle => "Idle",
            ProgressPhase::Preparing => "Preparing",
            ProgressPhase::Downloading => "Downloading",
            ProgressPhase::PostProcessing => "Post-processing",
            ProgressPhase::Finished => "Finished",
            ProgressPhase::Canceled => "Canceled",
            ProgressPhase::Failed => "Failed",
        }
    }

    fn update_from_line(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }

        if line.contains("[download] Destination:") || line.contains("[download] ") && line.contains("Destination:") {
            self.phase = ProgressPhase::Preparing;
            self.current_file = line.split("Destination:").nth(1).map(|text| text.trim().to_owned());
            return;
        }

        if let Some(after_tag) = line.strip_prefix("[download]") {
            self.phase = ProgressPhase::Downloading;
            let trimmed = after_tag.trim();

            if let Some(percent_token) = trimmed.split_whitespace().next()
                && let Some(percent_text) = percent_token.strip_suffix('%')
                && let Ok(percent) = percent_text.parse::<f32>()
            {
                self.percent = Some((percent / 100.0).clamp(0.0, 1.0));
            }

            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            for window in parts.windows(2) {
                match window {
                    ["at", speed] => self.speed = Some((*speed).to_owned()),
                    ["ETA", eta] => self.eta = Some((*eta).to_owned()),
                    _ => {}
                }
            }

            return;
        }

        if line.starts_with("[Merger]") || line.starts_with("[Metadata]") || line.starts_with("[EmbedThumbnail]") {
            self.phase = ProgressPhase::PostProcessing;
            return;
        }

        if line.contains("has already been downloaded") {
            self.phase = ProgressPhase::Finished;
            self.percent = Some(1.0);
            return;
        }
    }
}
