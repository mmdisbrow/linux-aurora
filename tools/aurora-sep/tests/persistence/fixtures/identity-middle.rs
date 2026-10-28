// SPDX-License-Identifier: GPL-2.0-only OR MIT

#[derive(Clone, Debug, PartialEq)]
pub enum EnrolOutcome {
    Done([u8; UUID_LEN]),
    Failed(u32),
}
#[derive(Clone, Debug)]
pub enum VerifyOutcome {
    Failed(u32),
}
#[derive(Clone, Copy, Default)]
pub enum Guidance {
    #[default]
    Place,
}
pub enum Op {
    Idle,
    Enrol {
        label: [u8; LABEL_LEN],
        stage: u32,
        terminal: Option<EnrolOutcome>,
        guidance: Guidance,
        percent: u32,
    },
    Verify {
        terminal: Option<VerifyOutcome>,
    },
}
impl Op {
    fn is_terminal(&self) -> bool {
        match self {
            Self::Enrol { terminal, .. } => terminal.is_some(),
            Self::Verify { terminal, .. } => terminal.is_some(),
            _ => false,
        }
    }
}
pub struct Session {
    pub generation: u64,
    pub worker_epoch: Option<u64>,
    pub open: bool,
    pub op: Op,
    pub unseen: bool,
    pub enrol_stage_unseen: bool,
    pub token: Option<u8>,
}
pub struct Context<'a> {
    pub session: &'a mut Session,
}
pub struct Handled {
    ret: i32,
    wake: bool,
    start_enrol: bool,
    start_verify: bool,
    delete_identity: Option<()>,
    delete_identities: KVec<()>,
}
fn ok() -> Result<Handled> {
    Ok(Handled {
        ret: 0,
        wake: false,
        start_enrol: false,
        start_verify: false,
        delete_identity: None,
        delete_identities: KVec::new(),
    })
}
type UserPtr = usize;
#[derive(Clone, Copy, Default, Debug, PartialEq)]
#[repr(u32)]
pub enum State {
    #[default]
    Idle = 0,
    Pending = 1,
    Progress = 2,
    Done = 3,
    Failed = 4,
}
#[derive(Clone, Default)]
pub struct EnrolPoll {
    stages_total: u32,
    stage: u32,
    guidance: Guidance,
    progress_percent: u32,
    pub state: State,
    pub status: u32,
    uuid: [u8; UUID_LEN],
}
struct UserSlice;
struct Writer;
impl UserSlice {
    fn new(_: usize, _: usize) -> Self {
        Self
    }
    fn writer(&self) -> Writer {
        Writer
    }
}
impl Writer {
    fn write(&mut self, out: &EnrolPoll) -> Result<()> {
        LAST_POLL.with(|v| *v.borrow_mut() = Some(out.clone()));
        Ok(())
    }
}
pub fn session() -> Session {
    Session {
        generation: 7,
        worker_epoch: Some(7),
        open: true,
        op: Op::Enrol {
            label: [9; LABEL_LEN],
            stage: 31,
            terminal: None,
            guidance: Guidance::Place,
            percent: 99,
        },
        unseen: false,
        enrol_stage_unseen: false,
        token: None,
    }
}
pub fn terminal(s: &Session) -> Option<EnrolOutcome> {
    match &s.op {
        Op::Enrol { terminal, .. } => terminal.clone(),
        _ => None,
    }
}
pub fn label(i: &IdentityIndex, uuid: [u8; 16]) -> Option<[u8; 128]> {
    i.entries.iter().find(|e| e.uuid == uuid).map(|e| e.label)
}
pub fn entry(uuid: u8, label: u8) -> Identity {
    Identity {
        uuid: [uuid; 16],
        label: [label; 128],
    }
}
