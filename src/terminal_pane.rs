use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tui_term::widget::PseudoTerminal;

#[derive(Debug)]
pub enum PaneEvent {
    Bell(String),
    Death(String),
}

pub struct TerminalPane {
    parser: Arc<Mutex<vt100::Parser>>,
    pty_writer: Box<dyn Write + Send>,
    #[allow(dead_code)]
    pty_master: Box<dyn portable_pty::MasterPty + Send>,
    reader_handle: Option<JoinHandle<()>>,
    #[allow(dead_code)]
    pub session_id: String,
    alive: Arc<AtomicBool>,
}

impl TerminalPane {
    pub fn new(
        session_id: String,
        attach_cmd: &str,
        attach_args: &[String],
        rows: u16,
        cols: u16,
        bell_tx: Sender<PaneEvent>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let pty_system = native_pty_system();
        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system.openpty(size)?;

        let mut cmd = CommandBuilder::new(attach_cmd);
        cmd.args(attach_args);

        pair.slave.spawn_command(cmd)?;
        // Drop the slave side — we only need the master
        drop(pair.slave);

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let alive = Arc::new(AtomicBool::new(true));

        let reader_handle = {
            let parser = Arc::clone(&parser);
            let alive = Arc::clone(&alive);
            let sid = session_id.clone();
            thread::spawn(move || {
                Self::reader_loop(reader, parser, alive, bell_tx, sid);
            })
        };

        Ok(TerminalPane {
            parser,
            pty_writer: writer,
            pty_master: pair.master,
            reader_handle: Some(reader_handle),
            session_id,
            alive,
        })
    }

    fn reader_loop(
        mut reader: Box<dyn Read + Send>,
        parser: Arc<Mutex<vt100::Parser>>,
        alive: Arc<AtomicBool>,
        bell_tx: Sender<PaneEvent>,
        session_id: String,
    ) {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => {
                    alive.store(false, Ordering::SeqCst);
                    let _ = bell_tx.send(PaneEvent::Death(session_id));
                    break;
                }
                Ok(n) => {
                    let data = &buf[..n];
                    if data.contains(&0x07) {
                        let _ = bell_tx.send(PaneEvent::Bell(session_id.clone()));
                    }
                    if let Ok(mut p) = parser.lock() {
                        p.process(data);
                    }
                }
            }
        }
    }

    pub fn write_input(&mut self, bytes: &[u8]) {
        let _ = self.pty_writer.write_all(bytes);
        let _ = self.pty_writer.flush();
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if let Ok(parser) = self.parser.lock() {
            let pseudo_term = PseudoTerminal::new(parser.screen());
            ratatui::widgets::Widget::render(pseudo_term, area, buf);
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        let _ = self.pty_master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        if let Ok(mut parser) = self.parser.lock() {
            parser.set_size(rows, cols);
        }
    }

}

impl Drop for TerminalPane {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        // Don't join the reader thread — it's blocked on read() and will exit
        // when the PTY master fd is closed (which happens when pty_master is dropped).
        // Joining here would deadlock since pty_master hasn't been dropped yet.
        self.reader_handle.take();
    }
}
