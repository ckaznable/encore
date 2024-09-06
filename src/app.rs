use std::time::Duration;

use eyre::Result;
use ratatui::{
    crossterm::event::{Event, EventStream, KeyCode, KeyEventKind},
    DefaultTerminal, Frame,
};
use tokio::time::interval;
use tokio_stream::StreamExt;

#[derive(Default)]
pub struct App {
    should_quit: bool,
    is_playing: bool,
}

impl App {
    pub async fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        let mut playing = interval(Duration::from_secs(1));
        let mut never = interval(Duration::from_secs(u64::MAX));
        let mut events = EventStream::new();

        while !self.should_quit {
            let tick = if self.is_playing {
                playing.tick()
            } else {
                never.tick()
            };

            tokio::select! {
                Some(Ok(event)) = events.next() => self.handle_event(&event),
                _ = tick => {
                    if !self.is_playing {
                        never.reset();
                        continue;
                    }

                    self.handle_tick();
                },
            }

            terminal.draw(|f| self.draw(f))?;
        }

        Ok(())
    }

    fn draw(&self, frame: &mut Frame) {}

    fn handle_event(&mut self, event: &Event) {
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
                    _ => {}
                }
            }
        }
    }

    fn handle_tick(&self) {}
}
