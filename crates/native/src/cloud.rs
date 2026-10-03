//! Optional desktop capability: look up the receiver's published gallery copies
//! in the user's Google Photos library and report verdicts back to it. Cookie
//! contents, paths and account details never appear in results or logs.
use super::*;
use backupduck_cloud_audit::{
    self as audit, Auditor, CookieFile, HashLookup, ItemInfo, Session, Throttle, Verdict,
};
use std::{collections::HashMap, future::Future};

/// Fits the default per-run RPC budget: 50 hashes or keys per call.
pub const DEFAULT_MAX_ITEMS: u32 = 600;

fn default_max_items() -> u32 {
    DEFAULT_MAX_ITEMS
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Command {
    Status {
        receiver_id: String,
        #[serde(default)]
        cookies_path: Option<PathBuf>,
    },
    Run {
        pairing: Pairing,
        cookies_path: PathBuf,
        #[serde(default)]
        account_index: u32,
        #[serde(default)]
        dry_run: bool,
        #[serde(default = "default_max_items")]
        max_items: u32,
    },
}

/// Google-side lookups, aligned with their inputs. Tests inject scripted fakes.
pub trait CloudLookup {
    fn lookup(
        &mut self,
        sha1_hex: &[String],
    ) -> impl Future<Output = audit::Result<Vec<Option<HashLookup>>>>;
    fn info(
        &mut self,
        media_keys: &[String],
    ) -> impl Future<Output = audit::Result<Vec<Option<ItemInfo>>>>;
}

/// Opens the session on first use, so a run with nothing due never contacts Google.
pub struct LiveLookup {
    cookies: CookieFile,
    account_index: u32,
    auditor: Option<Auditor>,
}
impl LiveLookup {
    pub fn new(cookies: CookieFile, account_index: u32) -> Self {
        Self {
            cookies,
            account_index,
            auditor: None,
        }
    }
    async fn auditor(&mut self) -> audit::Result<&mut Auditor> {
        if self.auditor.is_none() {
            let session = Session::open(self.cookies.jar(), self.account_index).await?;
            self.auditor = Some(Auditor::new(session, Throttle::default()));
        }
        Ok(self.auditor.as_mut().expect("auditor opened"))
    }
}
impl CloudLookup for LiveLookup {
    async fn lookup(&mut self, sha1_hex: &[String]) -> audit::Result<Vec<Option<HashLookup>>> {
        self.auditor().await?.lookup_hashes(sha1_hex).await
    }
    async fn info(&mut self, media_keys: &[String]) -> audit::Result<Vec<Option<ItemInfo>>> {
        self.auditor().await?.item_info(media_keys).await
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RunOptions {
    pub dry_run: bool,
    pub max_items: u32,
}

/// `verified`/`quota`/`not_found`/`unknown` count Google verdicts in this run;
/// receiver states follow from them per the protocol.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunReport {
    pub checked: u32,
    pub found: u32,
    pub verified: u32,
    pub quota: u32,
    /// Quota verdicts for copies not already known to count against quota.
    pub new_quota: u32,
    pub not_found: u32,
    pub unknown: u32,
    pub posted: u32,
    pub rejected: u32,
    pub dry_run: bool,
    pub session_expired: bool,
    /// Why the run ended early: `rate_limited`, `budget_exhausted` or an error code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<String>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

/// Stable codes only; audit errors can carry response fragments.
fn cloud_error(error: &audit::Error) -> Error {
    match error {
        audit::Error::SessionExpired => Error::Unsupported("cloud_session_expired".into()),
        audit::Error::RateLimited | audit::Error::BudgetExhausted => {
            Error::Unsupported("cloud_rate_limited".into())
        }
        audit::Error::Http(_) | audit::Error::Transport(_) => {
            Error::Transport("cloud lookup failed".into())
        }
        audit::Error::Parse(_) => Error::Unsupported("cloud_response_unrecognized".into()),
        audit::Error::Io(_) | audit::Error::Cookies(_) => {
            Error::Unsupported("cloud_cookies_invalid".into())
        }
    }
}

fn stop_reason(error: &audit::Error) -> &'static str {
    match error {
        audit::Error::SessionExpired => "session_expired",
        audit::Error::RateLimited => "rate_limited",
        audit::Error::BudgetExhausted => "budget_exhausted",
        other => error_code(cloud_error(other)),
    }
}

/// Verdicts aligned with `items`. An item-info failure still yields the page's
/// not-found verdicts; found items become unknown and the error ends the run.
async fn verdicts(
    lookup: &mut impl CloudLookup,
    items: &[CloudDueItem],
) -> audit::Result<(Vec<(Verdict, Option<HashLookup>)>, Option<audit::Error>)> {
    let hashes: Vec<String> = items.iter().map(|i| i.sha1.clone()).collect();
    let found = lookup.lookup(&hashes).await?;
    if found.len() != items.len() {
        return Err(audit::Error::Parse("hash lookup is not aligned".into()));
    }
    let keys: Vec<String> = found
        .iter()
        .flatten()
        .map(|f| f.media_key.clone())
        .collect();
    let (infos, stop) = if keys.is_empty() {
        (Vec::new(), None)
    } else {
        match lookup.info(&keys).await {
            Ok(infos) => (infos, None),
            Err(e) => (Vec::new(), Some(e)),
        }
    };
    let infos: HashMap<String, ItemInfo> = infos
        .into_iter()
        .flatten()
        .map(|i| (i.media_key.clone(), i))
        .collect();
    let verdicts = found
        .into_iter()
        .map(|hit| {
            let info = hit.as_ref().and_then(|h| infos.get(&h.media_key));
            (audit::verdict(hit.as_ref(), info), hit)
        })
        .collect();
    Ok((verdicts, stop))
}

impl SenderHost {
    fn require_cloud_audit(&self, receiver: &str) -> Result<()> {
        if self.sender.lock().map_err(lock)?.cloud_audit(receiver)? {
            Ok(())
        } else {
            Err(Error::Unsupported("cloud_audit_unsupported".into()))
        }
    }

    pub fn cloud_audit_status(&self, receiver: &str, cookies: Option<&Path>) -> Result<Value> {
        let run = self
            .sender
            .lock()
            .map_err(lock)?
            .cloud_audit_run(receiver)?;
        let session_expired = run
            .as_ref()
            .is_some_and(|(_, r)| r["session_expired"] == true);
        let (last_run_ms, last_result) = run.unzip();
        Ok(json!({
            "configured": cookies.is_some_and(Path::is_file),
            "last_run_ms": last_run_ms,
            "last_result": last_result,
            "session_expired": session_expired,
        }))
    }

    /// Checks due gallery copies, posts verdicts (unless `dry_run`) and mirrors
    /// them onto this sender's jobs. Unknown verdicts are not posted, so those
    /// copies stay pending and are looked up again next run.
    pub async fn cloud_audit(
        &self,
        pairing: &Pairing,
        lookup: &mut impl CloudLookup,
        options: RunOptions,
    ) -> Result<RunReport> {
        let receiver = pairing.receiver_id.as_str();
        self.require_cloud_audit(receiver)?;
        if self.cloud_audit_busy.swap(true, Ordering::SeqCst) {
            return Err(Error::Conflict("cloud audit already running".into()));
        }
        struct Guard<'a>(&'a AtomicBool);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::SeqCst);
            }
        }
        let _guard = Guard(&self.cloud_audit_busy);
        let client = pairing.client()?;
        let max = if options.max_items == 0 {
            DEFAULT_MAX_ITEMS
        } else {
            options.max_items
        };
        let mut report = RunReport {
            dry_run: options.dry_run,
            ..Default::default()
        };
        let mut after: Option<String> = None;
        let failure = loop {
            let remaining = max.saturating_sub(report.checked);
            if remaining == 0 {
                break None;
            }
            let page = client
                .cloud_due(
                    after.as_deref(),
                    remaining.min(MAX_CLOUD_ITEMS as u32),
                    false,
                )
                .await?;
            if page.items.is_empty() {
                break None;
            }
            let (results, stop) = match verdicts(lookup, &page.items).await {
                Ok(v) => v,
                Err(e) => break Some(e),
            };
            let now = now_ms();
            let mut observations = Vec::new();
            let mut mirrored = Vec::new();
            for (item, (verdict, hit)) in page.items.iter().zip(results) {
                report.checked += 1;
                report.found += u32::from(hit.is_some());
                let result = match verdict {
                    Verdict::Free => {
                        report.verified += 1;
                        CloudResult::Free
                    }
                    Verdict::CountsAgainstQuota => {
                        report.quota += 1;
                        report.new_quota +=
                            u32::from(item.cloud_state != "verified_counts_against_quota");
                        CloudResult::CountsAgainstQuota
                    }
                    Verdict::NotFound => {
                        report.not_found += 1;
                        CloudResult::NotFound
                    }
                    Verdict::Unknown => {
                        report.unknown += 1;
                        continue;
                    }
                };
                let (media_key, device_model) = hit.map(|h| (h.media_key, h.device_model)).unzip();
                let mut observation = CloudObservation {
                    asset_id: item.asset_id.clone(),
                    sha1: item.sha1.clone(),
                    result,
                    media_key,
                    device_model: device_model.flatten(),
                };
                if observation.validate().is_err() {
                    // Labels are optional; never let an odd one sink the batch.
                    observation.media_key = None;
                    observation.device_model = None;
                }
                mirrored.push(
                    next_cloud_state(&item.cloud_state, result, item.published_at_ms, now)
                        .to_owned(),
                );
                observations.push(observation);
            }
            if !options.dry_run && !observations.is_empty() {
                for chunk in observations.chunks(MAX_CLOUD_ITEMS) {
                    let summary = client.post_cloud_observations(chunk).await?;
                    report.posted += chunk.len() as u32;
                    report.rejected += summary.rejected;
                }
                let mut sender = self.sender.lock().map_err(lock)?;
                for (o, state) in observations.iter().zip(&mirrored) {
                    sender.observe_cloud(
                        receiver,
                        &o.asset_id,
                        state,
                        o.device_model.as_deref(),
                        now,
                    )?;
                }
            }
            if stop.is_some() {
                break stop;
            }
            match page.next {
                Some(next) => after = Some(next),
                None => break None,
            }
        };
        if let Some(error) = &failure {
            report.session_expired = matches!(error, audit::Error::SessionExpired);
            if !report.session_expired {
                report.stopped = Some(stop_reason(error).into());
            }
        }
        self.sender.lock().map_err(lock)?.record_cloud_audit_run(
            receiver,
            now_ms(),
            &serde_json::to_value(&report)?,
        )?;
        self.maintenance.lock().map_err(lock)?.log(
            if report.session_expired {
                "cloud_session_expired"
            } else {
                "cloud_audit_run"
            },
            None,
            None,
        )?;
        match failure {
            Some(error) if report.checked == 0 => Err(cloud_error(&error)),
            _ => Ok(report),
        }
    }
}

pub(super) fn call(command: Command) -> Result<Value> {
    let host = sender()?;
    match command {
        Command::Status {
            receiver_id,
            cookies_path,
        } => host.cloud_audit_status(&receiver_id, cookies_path.as_deref()),
        Command::Run {
            pairing,
            cookies_path,
            account_index,
            dry_run,
            max_items,
        } => {
            host.require_cloud_audit(&pairing.receiver_id)?;
            let cookies = CookieFile::load(&cookies_path).map_err(|e| cloud_error(&e))?;
            let mut lookup = LiveLookup::new(cookies, account_index);
            let report = runtime().block_on(host.cloud_audit(
                &pairing,
                &mut lookup,
                RunOptions { dry_run, max_items },
            ))?;
            Ok(serde_json::to_value(report)?)
        }
    }
}
