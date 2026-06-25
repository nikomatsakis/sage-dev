use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use crate::symbol::{CrateNum, DefIndex};

use super::{ExternalDefPath, RawChild, TcxDb};

/// Request from the salsa thread to the TyCtxt thread.
/// Each variant carries a oneshot sender for its typed response.
pub enum TcxRequest {
    ExternCrate {
        name: String,
        reply: mpsc::Sender<Option<CrateNum>>,
    },
    ModuleChildren {
        crate_num: CrateNum,
        def_index: DefIndex,
        reply: mpsc::Sender<Vec<RawChild>>,
    },
    ItemName {
        crate_num: CrateNum,
        def_index: DefIndex,
        reply: mpsc::Sender<Option<String>>,
    },
    IsModule {
        crate_num: CrateNum,
        def_index: DefIndex,
        reply: mpsc::Sender<bool>,
    },
    IsBuiltinDerive {
        crate_num: CrateNum,
        def_index: DefIndex,
        reply: mpsc::Sender<bool>,
    },
    DefPath {
        crate_num: CrateNum,
        def_index: DefIndex,
        reply: mpsc::Sender<Option<String>>,
    },
    StructuredDefPath {
        crate_num: CrateNum,
        def_index: DefIndex,
        reply: mpsc::Sender<Option<ExternalDefPath>>,
    },
    ExpandDerive {
        crate_num: CrateNum,
        def_index: DefIndex,
        item_source: String,
        reply: mpsc::Sender<Option<String>>,
    },
    ExpandBang {
        crate_num: CrateNum,
        def_index: DefIndex,
        input_tokens: String,
        reply: mpsc::Sender<Option<String>>,
    },
    ExpandAttr {
        crate_num: CrateNum,
        def_index: DefIndex,
        attr_args: String,
        item_source: String,
        reply: mpsc::Sender<Option<String>>,
    },
}

/// Channel-based `TcxDb` proxy. Sends requests to the thread that owns
/// `TyCtxt<'tcx>` and blocks for typed responses. Fully `'static` and `Send + Sync`.
#[derive(Clone)]
pub struct ProxyTcxDb {
    tx: mpsc::Sender<TcxRequest>,
    log: Arc<Mutex<Vec<String>>>,
}

impl ProxyTcxDb {
    pub fn new(tx: mpsc::Sender<TcxRequest>, log: Arc<Mutex<Vec<String>>>) -> Self {
        Self { tx, log }
    }
}

impl TcxDb for ProxyTcxDb {
    fn extern_crate(&self, name: &str) -> Option<CrateNum> {
        self.log
            .lock()
            .unwrap()
            .push(format!("tcx::extern_crate(\"{name}\")"));
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(TcxRequest::ExternCrate {
                name: name.to_owned(),
                reply,
            })
            .expect("TyCtxt thread hung up");
        rx.recv().expect("TyCtxt thread hung up")
    }

    fn module_children(&self, crate_num: CrateNum, def_index: DefIndex) -> Vec<RawChild> {
        self.log.lock().unwrap().push(format!(
            "tcx::module_children({}, {})",
            crate_num.0, def_index.0
        ));
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(TcxRequest::ModuleChildren {
                crate_num,
                def_index,
                reply,
            })
            .expect("TyCtxt thread hung up");
        rx.recv().expect("TyCtxt thread hung up")
    }

    fn item_name(&self, crate_num: CrateNum, def_index: DefIndex) -> Option<String> {
        self.log
            .lock()
            .unwrap()
            .push(format!("tcx::item_name({}, {})", crate_num.0, def_index.0));
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(TcxRequest::ItemName {
                crate_num,
                def_index,
                reply,
            })
            .expect("TyCtxt thread hung up");
        rx.recv().expect("TyCtxt thread hung up")
    }

    fn is_builtin_derive(&self, crate_num: CrateNum, def_index: DefIndex) -> bool {
        self.log.lock().unwrap().push(format!(
            "tcx::is_builtin_derive({}, {})",
            crate_num.0, def_index.0
        ));
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(TcxRequest::IsBuiltinDerive {
                crate_num,
                def_index,
                reply,
            })
            .expect("TyCtxt thread hung up");
        rx.recv().expect("TyCtxt thread hung up")
    }

    fn is_module(&self, crate_num: CrateNum, def_index: DefIndex) -> bool {
        self.log
            .lock()
            .unwrap()
            .push(format!("tcx::is_module({}, {})", crate_num.0, def_index.0));
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(TcxRequest::IsModule {
                crate_num,
                def_index,
                reply,
            })
            .expect("TyCtxt thread hung up");
        rx.recv().expect("TyCtxt thread hung up")
    }

    fn def_path(&self, crate_num: CrateNum, def_index: DefIndex) -> Option<String> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(TcxRequest::DefPath {
                crate_num,
                def_index,
                reply,
            })
            .expect("TyCtxt thread hung up");
        rx.recv().expect("TyCtxt thread hung up")
    }

    fn structured_def_path(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
    ) -> Option<ExternalDefPath> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(TcxRequest::StructuredDefPath {
                crate_num,
                def_index,
                reply,
            })
            .expect("TyCtxt thread hung up");
        rx.recv().expect("TyCtxt thread hung up")
    }

    fn expand_proc_macro_derive(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
        item_source: &str,
    ) -> Option<String> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(TcxRequest::ExpandDerive {
                crate_num,
                def_index,
                item_source: item_source.to_owned(),
                reply,
            })
            .expect("TyCtxt thread hung up");
        rx.recv().expect("TyCtxt thread hung up")
    }

    fn expand_proc_macro_bang(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
        input_tokens: &str,
    ) -> Option<String> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(TcxRequest::ExpandBang {
                crate_num,
                def_index,
                input_tokens: input_tokens.to_owned(),
                reply,
            })
            .expect("TyCtxt thread hung up");
        rx.recv().expect("TyCtxt thread hung up")
    }

    fn expand_proc_macro_attr(
        &self,
        crate_num: CrateNum,
        def_index: DefIndex,
        attr_args: &str,
        item_source: &str,
    ) -> Option<String> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(TcxRequest::ExpandAttr {
                crate_num,
                def_index,
                attr_args: attr_args.to_owned(),
                item_source: item_source.to_owned(),
                reply,
            })
            .expect("TyCtxt thread hung up");
        rx.recv().expect("TyCtxt thread hung up")
    }
}
