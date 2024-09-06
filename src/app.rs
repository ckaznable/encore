use eyre::Result;
use ratatui::DefaultTerminal;

#[derive(Default)]
pub struct App {}

impl App {
    pub async fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        Ok(())
    }
}
