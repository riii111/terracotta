#[derive(Default)]
pub(super) struct LineBuffer {
    pending: Vec<u8>,
    scanned: usize,
}

impl LineBuffer {
    pub(super) fn push(&mut self, bytes: &[u8], mut on_line: impl FnMut(&[u8])) {
        self.pending.extend_from_slice(bytes);

        let mut line_start = 0;
        let mut scan_start = self.scanned;
        while let Some(relative_newline) = self.pending[scan_start..]
            .iter()
            .position(|byte| *byte == b'\n')
        {
            let newline = scan_start + relative_newline;
            let mut line_end = newline;
            if line_end > line_start && self.pending[line_end - 1] == b'\r' {
                line_end -= 1;
            }
            on_line(&self.pending[line_start..line_end]);
            line_start = newline + 1;
            scan_start = line_start;
        }

        if line_start == 0 {
            self.scanned = self.pending.len();
        } else {
            self.pending.drain(..line_start);
            self.scanned = 0;
        }
    }

    pub(super) fn finish(&mut self) -> Vec<u8> {
        self.scanned = 0;
        std::mem::take(&mut self.pending)
    }
}
