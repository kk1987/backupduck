//! Optional desktop capability: look up the receiver's published gallery copies
//! in the user's Google Photos library and report verdicts back to it. Cookie
//! contents, paths and account details never appear in results or logs.
use super::*;
use backupduck_cloud_audit::{
    self as audit, auditor::CHUNK, Auditor, CookieFile, HashLookup, ItemInfo, LockedMove, Session,
    Throttle, Verdict,
};
use backupduck_sender::LockedFolderJob;
use std::{
    collections::{HashMap, HashSet},
    future::Future,
};

/// Fits the default per-run RPC budget: 50 hashes or keys per call.
pub const DEFAULT_MAX_ITEMS: u32 = 600;

fn default_max_items() -> u32 {
    DEFAULT_MAX_ITEMS
}

/// Locked Folder moves per run: three RPCs per 50 items, well inside the budget.
pub const DEFAULT_LOCK_MAX_ITEMS: u32 = 200;
/// Failed moves are retried on later runs up to this many attempts.
pub const MAX_LOCK_ATTEMPTS: u32 = 3;

fn default_lock_max_items() -> u32 {
    DEFAULT_LOCK_MAX_ITEMS
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
    /// Moves cloud-verified copies of hidden sources into the Google Photos
    /// Locked Folder. The host decides hiddenness: `source_ids` are the
    /// PhotoKit identifiers of currently hidden assets.
    LockHidden {
        pairing: Pairing,
        cookies_path: PathBuf,
        #[serde(default)]
        account_index: u32,
        source_ids: Vec<String>,
        #[serde(default)]
        dry_run: bool,
        #[serde(default = "default_lock_max_items")]
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
    /// Moves library items (by dedup key) into the Locked Folder.
    fn move_to_locked(
        &mut self,
        dedup_keys: &[String],
    ) -> impl Future<Output = audit::Result<LockedMove>>;
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
    async fn move_to_locked(&mut self, dedup_keys: &[String]) -> audit::Result<LockedMove> {
        self.auditor()
            .await?
            .move_to_locked_folder(dedup_keys)
            .await
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

#[derive(Clone, Debug)]
pub struct LockOptions {
    pub source_ids: Vec<String>,
    pub dry_run: bool,
    pub max_items: u32,
}

/// One Locked Folder run. `moved`, `failed` and `not_in_library` are outcomes
/// of this run; `already_moved` and `attempts_exhausted` are earlier ones.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockReport {
    /// Received jobs of hidden sources whose copy is `verified`, including
    /// those already moved.
    pub verified_hidden: u32,
    /// Verified items selected this run (at most `max_items`).
    pub candidates: u32,
    /// Verified items left for a later run by `max_items`.
    pub deferred: u32,
    /// Candidates looked up in Google Photos.
    pub checked: u32,
    /// Candidates still in the library before moving (what a dry run would move).
    pub found: u32,
    pub moved: u32,
    pub failed: u32,
    pub not_in_library: u32,
    pub already_moved: u32,
    pub attempts_exhausted: u32,
    pub skipped_quota: u32,
    pub skipped_unverified: u32,
    /// Verified but the receiver has no SHA-1 evidence for the copy.
    pub skipped_no_sha1: u32,
    pub dry_run: bool,
    pub session_expired: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<String>,
}

/// Whether a failed `StLnCe` call may still have taken effect. Session,
/// rate-limit and budget failures never reached Google's move handler.
fn move_may_have_happened(error: &audit::Error) -> bool {
    !matches!(
        error,
        audit::Error::SessionExpired | audit::Error::RateLimited | audit::Error::BudgetExhausted
    )
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

struct BusyGuard<'a>(&'a AtomicBool);
impl<'a> BusyGuard<'a> {
    fn acquire(flag: &'a AtomicBool) -> Result<Self> {
        if flag.swap(true, Ordering::SeqCst) {
            return Err(Error::Conflict("cloud audit already running".into()));
        }
        Ok(Self(flag))
    }
}
impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
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
        let (last_lock_run_ms, last_lock_result) = self
            .sender
            .lock()
            .map_err(lock)?
            .locked_folder_run(receiver)?
            .unzip();
        Ok(json!({
            "configured": cookies.is_some_and(Path::is_file),
            "last_run_ms": last_run_ms,
            "last_result": last_result,
            "session_expired": session_expired,
            "last_lock_run_ms": last_lock_run_ms,
            "last_lock_result": last_lock_result,
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
        let _guard = BusyGuard::acquire(&self.cloud_audit_busy)?;
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

impl SenderHost {
    /// Moves cloud-verified copies of the given hidden sources into the Google
    /// Photos Locked Folder, at most `max_items` per run. Only received jobs
    /// whose copy is `verified` qualify. A move counts only when `StLnCe`
    /// succeeded and a fresh lookup no longer finds the SHA-1 in the library.
    /// Receiver state is not touched: its `verified` rows are never re-checked.
    pub async fn lock_hidden(
        &self,
        pairing: &Pairing,
        lookup: &mut impl CloudLookup,
        options: LockOptions,
    ) -> Result<LockReport> {
        let receiver = pairing.receiver_id.as_str();
        self.require_cloud_audit(receiver)?;
        let _guard = BusyGuard::acquire(&self.cloud_audit_busy)?;
        let max = if options.max_items == 0 {
            DEFAULT_LOCK_MAX_ITEMS
        } else {
            options.max_items
        } as usize;
        let mut report = LockReport {
            dry_run: options.dry_run,
            ..Default::default()
        };
        let jobs = self
            .sender
            .lock()
            .map_err(lock)?
            .locked_folder_jobs(receiver, &options.source_ids)?;
        let mut eligible = Vec::new();
        for job in jobs {
            match job.cloud.as_deref() {
                Some("verified") => report.verified_hidden += 1,
                Some("verified_counts_against_quota") => {
                    report.skipped_quota += 1;
                    continue;
                }
                _ => {
                    report.skipped_unverified += 1;
                    continue;
                }
            }
            match job.status.as_deref() {
                Some("moved") => report.already_moved += 1,
                // Never moved and gone from the library: nothing left to do.
                Some("not_in_library") => {}
                Some("failed") if job.attempts >= MAX_LOCK_ATTEMPTS => {
                    report.attempts_exhausted += 1
                }
                _ => eligible.push(job),
            }
        }
        report.deferred = eligible.len().saturating_sub(max) as u32;
        eligible.truncate(max);
        report.candidates = eligible.len() as u32;

        let failure = if eligible.is_empty() {
            None
        } else {
            let sha1 = self.gallery_sha1(pairing, &eligible).await?;
            let mut items = Vec::new();
            for job in eligible {
                match sha1.get(&job.asset_id) {
                    Some(sha1) => items.push((job, sha1.clone())),
                    None => report.skipped_no_sha1 += 1,
                }
            }
            self.lock_items(receiver, lookup, &items, &mut report)
                .await?
        };
        if let Some(error) = &failure {
            report.session_expired = matches!(error, audit::Error::SessionExpired);
            if !report.session_expired {
                report.stopped = Some(stop_reason(error).into());
            }
        }
        self.sender.lock().map_err(lock)?.record_locked_folder_run(
            receiver,
            now_ms(),
            &serde_json::to_value(&report)?,
        )?;
        self.maintenance.lock().map_err(lock)?.log(
            if report.session_expired {
                "cloud_session_expired"
            } else {
                "cloud_locked_folder_run"
            },
            None,
            None,
        )?;
        match failure {
            Some(error) if report.checked == 0 => Err(cloud_error(&error)),
            _ => Ok(report),
        }
    }

    /// SHA-1 of each job's gallery copy from the receiver's evidence, which
    /// outlives the phone copy. Pages stop once every asset is found.
    async fn gallery_sha1(
        &self,
        pairing: &Pairing,
        jobs: &[LockedFolderJob],
    ) -> Result<HashMap<String, String>> {
        let client = pairing.client()?;
        let mut wanted: HashSet<&str> = jobs.iter().map(|j| j.asset_id.as_str()).collect();
        let mut found = HashMap::new();
        let mut after: Option<String> = None;
        while !wanted.is_empty() {
            let page = client
                .cloud_due(after.as_deref(), MAX_CLOUD_ITEMS as u32, true)
                .await?;
            for item in page.items {
                if wanted.remove(item.asset_id.as_str()) {
                    found.insert(item.asset_id, item.sha1);
                }
            }
            match page.next {
                Some(next) => after = Some(next),
                None => break,
            }
        }
        Ok(found)
    }

    /// Looks up, moves and re-checks `items` in batches. Returns the Google
    /// error that ended the run early, if any.
    async fn lock_items(
        &self,
        receiver: &str,
        lookup: &mut impl CloudLookup,
        items: &[(LockedFolderJob, String)],
        report: &mut LockReport,
    ) -> Result<Option<audit::Error>> {
        let record = |job: &LockedFolderJob, status: &str, key: Option<&str>, attempt: bool| {
            self.sender.lock().map_err(lock)?.record_locked_folder(
                receiver,
                &job.asset_id,
                status,
                key,
                attempt,
                now_ms(),
            )
        };
        for batch in items.chunks(CHUNK) {
            let hashes: Vec<String> = batch.iter().map(|(_, sha1)| sha1.clone()).collect();
            let hits = match lookup.lookup(&hashes).await {
                Ok(hits) if hits.len() == batch.len() => hits,
                Ok(_) => {
                    return Ok(Some(audit::Error::Parse(
                        "hash lookup is not aligned".into(),
                    )))
                }
                Err(e) => return Ok(Some(e)),
            };
            report.checked += batch.len() as u32;
            let dry_run = report.dry_run;
            let mut to_move = Vec::new();
            for ((job, sha1), hit) in batch.iter().zip(hits) {
                match hit {
                    // A move requested earlier whose re-check did not finish.
                    None if job.dedup_key.is_some() => {
                        if dry_run {
                            report.already_moved += 1;
                        } else {
                            report.moved += 1;
                            record(job, "moved", None, false)?;
                        }
                    }
                    None => {
                        report.not_in_library += 1;
                        if !dry_run {
                            record(job, "not_in_library", None, false)?;
                        }
                    }
                    Some(hit) => {
                        report.found += 1;
                        match hit.dedup_key.filter(|k| !k.is_empty()) {
                            Some(key) => to_move.push((job, key, sha1)),
                            None if dry_run => {}
                            None => {
                                report.failed += 1;
                                record(job, "failed", None, true)?;
                            }
                        }
                    }
                }
            }
            if dry_run || to_move.is_empty() {
                continue;
            }
            let keys: Vec<String> = to_move.iter().map(|(_, k, _)| k.clone()).collect();
            if let Err(e) = lookup.move_to_locked(&keys).await {
                // Only a call that may have reached Google counts; the next
                // run's lookup settles whether it took effect.
                if move_may_have_happened(&e) {
                    for (job, key, _) in &to_move {
                        report.failed += 1;
                        record(job, "failed", Some(key), true)?;
                    }
                }
                return Ok(Some(e));
            }
            let hashes: Vec<String> = to_move.iter().map(|(_, _, h)| (*h).clone()).collect();
            let recheck = match lookup.lookup(&hashes).await {
                Ok(hits) if hits.len() == to_move.len() => Ok(hits),
                Ok(_) => Err(audit::Error::Parse("hash lookup is not aligned".into())),
                Err(e) => Err(e),
            };
            match recheck {
                Ok(hits) => {
                    for ((job, key, _), hit) in to_move.iter().zip(hits) {
                        if hit.is_none() {
                            report.moved += 1;
                            record(job, "moved", Some(key), false)?;
                        } else {
                            report.failed += 1;
                            record(job, "failed", Some(key), true)?;
                        }
                    }
                }
                Err(e) => {
                    // The move went through; the next run settles each item.
                    for (job, key, _) in &to_move {
                        report.failed += 1;
                        record(job, "failed", Some(key), false)?;
                    }
                    return Ok(Some(e));
                }
            }
        }
        Ok(None)
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
        Command::LockHidden {
            pairing,
            cookies_path,
            account_index,
            source_ids,
            dry_run,
            max_items,
        } => {
            host.require_cloud_audit(&pairing.receiver_id)?;
            let cookies = CookieFile::load(&cookies_path).map_err(|e| cloud_error(&e))?;
            let mut lookup = LiveLookup::new(cookies, account_index);
            let report = runtime().block_on(host.lock_hidden(
                &pairing,
                &mut lookup,
                LockOptions {
                    source_ids,
                    dry_run,
                    max_items,
                },
            ))?;
            Ok(serde_json::to_value(report)?)
        }
    }
}
