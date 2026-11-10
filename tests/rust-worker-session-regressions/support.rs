use std::time::Duration;
pub use zipelo_rust_worker_session_model::{
    ApplyError, Channel, ExitStatus, Fault, Limits, MAX_DATA_BYTES, MAX_DIAGNOSTIC_BYTES,
    MAX_FRAMES, MAX_OBSERVATIONS, MAX_WIRE_BYTES, ModelReport, Observation, Outcome, Phase,
    SessionModel,
    wire::{
        CHUNK_BYTES, HEADER_BYTES, Header, Kind, ProtocolError, RequestId, WorkerStatus,
        encode_header,
    },
};
pub const ZERO: Duration = Duration::ZERO;

pub fn id() -> RequestId {
    RequestId::new([19; 16]).unwrap()
}
pub fn model() -> SessionModel<'static> {
    SessionModel::new(id(), b"", Limits::default()).unwrap()
}
pub fn authority(report: ModelReport) {
    assert!(!report.live_verified());
    assert!(!report.publication_allowed());
}
pub fn apply(
    m: &mut SessionModel<'_>,
    at: Duration,
    event: Observation<'_>,
) -> Result<(), ApplyError> {
    let result = m.observe(at, event);
    authority(m.report());
    result
}
pub fn ack(m: &mut SessionModel<'_>, at: Duration, sequence: u64) -> Result<(), ApplyError> {
    let result = m.request_frame_sent(at, sequence);
    authority(m.report());
    result
}
pub fn sent(m: &mut SessionModel<'_>) {
    while let Some(f) = m.outbound() {
        let sequence = Header::inspect(&f.header).unwrap().sequence();
        ack(m, ZERO, sequence).unwrap();
    }
}
pub fn raw(
    m: &mut SessionModel<'_>,
    header: &[u8; HEADER_BYTES],
    payload: &[u8],
) -> Result<(), ApplyError> {
    apply(m, ZERO, Observation::ResultFrame { header, payload })
}
pub fn frame(
    m: &mut SessionModel<'_>,
    kind: Kind,
    seq: u64,
    bytes: &[u8],
) -> Result<(), ApplyError> {
    raw(
        m,
        &encode_header(kind, id(), seq, bytes.len() as u32),
        bytes,
    )
}
pub fn begin(m: &mut SessionModel<'_>, length: u64) {
    frame(m, Kind::ResultBegin, 0, &length.to_le_bytes()).unwrap();
}
pub fn complete(m: &mut SessionModel<'_>) {
    sent(m);
    begin(m, 0);
    frame(m, Kind::ResultEnd, 1, &[1]).unwrap();
}
pub fn cleanup(m: &mut SessionModel<'_>, code: u32) {
    for e in [
        Observation::Closed(Channel::Request),
        Observation::ResultEof,
        Observation::DiagnosticEof,
        Observation::Exit(ExitStatus::Code(code)),
        Observation::Reaped(ExitStatus::Code(code)),
        Observation::Closed(Channel::Result),
        Observation::Closed(Channel::Diagnostic),
        Observation::ResourcesReleased,
    ] {
        apply(m, ZERO, e).unwrap();
    }
}
pub fn fault(m: &SessionModel<'_>, expected: Fault) {
    let r = m.report();
    authority(r);
    assert_eq!(r.first_fault, Some(expected));
    assert_eq!(r.outcome, Outcome::Failed(expected));
}
pub fn protocol(m: &SessionModel<'_>, expected: ProtocolError) {
    fault(m, Fault::Protocol(expected));
}
pub fn blocked(m: &SessionModel<'_>) {
    authority(m.report());
    assert_eq!(m.report().phase, Phase::Blocked);
    assert!(m.report().observations_unreliable);
    assert!(m.outbound().is_none());
}
