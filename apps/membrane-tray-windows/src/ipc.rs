use membrane_protocol::{decode_event_frame, DaemonEventV1, DaemonProtocolError};

#[derive(Debug, Default)]
pub struct EventDecoder {
    last_sequence: Option<u64>,
}
impl EventDecoder {
    pub fn decode(&mut self, frame: &[u8]) -> Result<DaemonEventV1, DaemonProtocolError> {
        let event = decode_event_frame(frame, self.last_sequence)?;
        self.last_sequence = Some(event.sequence);
        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use membrane_protocol::{encode_frame, DaemonEventKind};
    #[test]
    fn rejects_sequence_regression() {
        let event = DaemonEventV1 {
            schema_version: 1,
            sequence: 1,
            kind: DaemonEventKind::Ready,
            pid: 9,
            observed_at_unix_ms: 1,
            endpoint: None,
            reason: None,
        };
        let frame = encode_frame(&event).unwrap();
        let mut decoder = EventDecoder::default();
        decoder.decode(&frame).unwrap();
        assert!(decoder.decode(&frame).is_err());
    }
}
