use std::{
    any::Any,
    fmt::{Debug, Display},
    future::Future,
    hash::Hash,
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    task::Poll,
};

use anyhow::Result;
use event_listener::EventListener;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    backend::CellContent,
    manager::{
        read_task_cell, read_task_cell_untracked, read_task_output, read_task_output_untracked,
        TurboTasksApi,
    },
    registry::get_value_type,
    turbo_tasks, SharedReference, TaskId, TraitTypeId, ValueTypeId,
};

/// The result of reading a ValueVc.
/// Can be dereferenced to the concrete type.
///
/// This type is needed because the content of the cell can change
/// concurrently, so we can't return a reference to the data directly.
/// Instead the data or an [Arc] to the data is cloned out of the cell
/// and hold by this type.
pub struct RawVcReadResult<T: Any + Send + Sync> {
    inner: Arc<T>,
}

impl<T: Any + Send + Sync> RawVcReadResult<T> {
    pub(crate) fn new(shared_ref: Arc<T>) -> Self {
        Self { inner: shared_ref }
    }

    fn map_read_result<O, F: Fn(&T) -> &O>(self, func: F) -> RawVcReadAndMapResult<T, O, F> {
        RawVcReadAndMapResult {
            inner: self.inner,
            func,
        }
    }

    pub fn into_arc(self) -> Arc<T> {
        self.inner
    }
}

impl<T: Any + Send + Sync> std::ops::Deref for RawVcReadResult<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: Display + Any + Send + Sync> Display for RawVcReadResult<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&**self, f)
    }
}

impl<T: Debug + Any + Send + Sync> Debug for RawVcReadResult<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&**self, f)
    }
}

pub struct RawVcReadAndMapResult<T: Any + Send + Sync, O, F: Fn(&T) -> &O> {
    inner: Arc<T>,
    func: F,
}

impl<T: Any + Send + Sync, O, F: Fn(&T) -> &O> std::ops::Deref for RawVcReadAndMapResult<T, O, F> {
    type Target = O;

    fn deref(&self) -> &Self::Target {
        (self.func)(&*self.inner)
    }
}

impl<T: Any + Send + Sync, O: Display, F: Fn(&T) -> &O> Display for RawVcReadAndMapResult<T, O, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&**self, f)
    }
}

impl<T: Any + Send + Sync, O: Debug, F: Fn(&T) -> &O> Debug for RawVcReadAndMapResult<T, O, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&**self, f)
    }
}

#[derive(Error, Debug)]
pub enum ResolveTypeError {
    #[error("no content in the cell")]
    NoContent,
    #[error("the content in the cell has no type")]
    UntypedContent,
    #[error("content is not available as task execution failed")]
    TaskError { source: anyhow::Error },
    #[error("reading the cell content failed")]
    ReadError { source: anyhow::Error },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RawVc {
    TaskOutput(TaskId),
    TaskCell(TaskId, usize),
}

impl RawVc {
    pub fn into_read<T: Any + Send + Sync>(self) -> ReadRawVcFuture<T> {
        // returns a custom future to have something concrete and sized
        // this avoids boxing in IntoFuture
        ReadRawVcFuture::new(self)
    }

    pub fn into_strongly_consistent_read<T: Any + Send + Sync>(self) -> ReadRawVcFuture<T> {
        // returns a custom future to have something concrete and sized
        // this avoids boxing in IntoFuture
        ReadRawVcFuture::new_strongly_consistent(self)
    }

    /// INVALIDATION: Be careful with this, it will not track dependencies, so
    /// using it could break cache invalidation.
    pub async fn into_read_untracked<T: Any + Send + Sync>(
        self,
        turbo_tasks: &dyn TurboTasksApi,
    ) -> Result<RawVcReadResult<T>> {
        self.into_read_untracked_internal(false, turbo_tasks).await
    }

    /// INVALIDATION: Be careful with this, it will not track dependencies, so
    /// using it could break cache invalidation.
    pub async fn into_strongly_consistent_read_untracked<T: Any + Send + Sync>(
        self,
        turbo_tasks: &dyn TurboTasksApi,
    ) -> Result<RawVcReadResult<T>> {
        self.into_read_untracked_internal(true, turbo_tasks).await
    }

    async fn into_read_untracked_internal<T: Any + Send + Sync>(
        self,
        strongly_consistent: bool,
        turbo_tasks: &dyn TurboTasksApi,
    ) -> Result<RawVcReadResult<T>> {
        turbo_tasks.notify_scheduled_tasks();
        let mut current = self;
        loop {
            match current {
                RawVc::TaskOutput(task) => {
                    current =
                        read_task_output_untracked(turbo_tasks, task, strongly_consistent).await?
                }
                RawVc::TaskCell(task, index) => {
                    return read_task_cell_untracked(turbo_tasks, task, index)
                        .await?
                        .cast::<T>();
                }
            }
        }
    }

    pub async fn resolve_trait(
        self,
        trait_type: TraitTypeId,
    ) -> Result<Option<RawVc>, ResolveTypeError> {
        let tt = turbo_tasks();
        tt.notify_scheduled_tasks();
        let mut current = self;
        loop {
            match current {
                RawVc::TaskOutput(task) => {
                    current = read_task_output(&*tt, task, false)
                        .await
                        .map_err(|source| ResolveTypeError::TaskError { source })?;
                }
                RawVc::TaskCell(task, index) => {
                    let content = read_task_cell(&*tt, task, index)
                        .await
                        .map_err(|source| ResolveTypeError::ReadError { source })?;
                    if let CellContent(Some(shared_reference)) = content {
                        if let SharedReference(Some(value_type), _) = shared_reference {
                            if get_value_type(value_type).traits.contains(&trait_type) {
                                return Ok(Some(RawVc::TaskCell(task, index)));
                            } else {
                                return Ok(None);
                            }
                        } else {
                            return Err(ResolveTypeError::UntypedContent);
                        }
                    } else {
                        return Err(ResolveTypeError::NoContent);
                    }
                }
            }
        }
    }

    pub async fn resolve_value(
        self,
        value_type: ValueTypeId,
    ) -> Result<Option<RawVc>, ResolveTypeError> {
        let tt = turbo_tasks();
        tt.notify_scheduled_tasks();
        let mut current = self;
        loop {
            match current {
                RawVc::TaskOutput(task) => {
                    current = read_task_output(&*tt, task, false)
                        .await
                        .map_err(|source| ResolveTypeError::TaskError { source })?;
                }
                RawVc::TaskCell(task, index) => {
                    let content = read_task_cell(&*tt, task, index)
                        .await
                        .map_err(|source| ResolveTypeError::ReadError { source })?;
                    if let CellContent(Some(shared_reference)) = content {
                        if let SharedReference(Some(cell_value_type), _) = shared_reference {
                            if cell_value_type == value_type {
                                return Ok(Some(RawVc::TaskCell(task, index)));
                            } else {
                                return Ok(None);
                            }
                        } else {
                            return Err(ResolveTypeError::UntypedContent);
                        }
                    } else {
                        return Err(ResolveTypeError::NoContent);
                    }
                }
            }
        }
    }

    pub async fn resolve(self) -> Result<RawVc> {
        let tt = turbo_tasks();
        let mut current = self;
        let mut notified = false;
        loop {
            match current {
                RawVc::TaskOutput(task) => {
                    if !notified {
                        tt.notify_scheduled_tasks();
                        notified = true;
                    }
                    current = read_task_output(&*tt, task, false).await?;
                }
                RawVc::TaskCell(_, _) => return Ok(current),
            }
        }
    }

    pub fn is_resolved(&self) -> bool {
        match self {
            RawVc::TaskOutput(_) => false,
            RawVc::TaskCell(_, _) => true,
        }
    }

    pub fn get_task_id(&self) -> TaskId {
        match self {
            RawVc::TaskOutput(t) | RawVc::TaskCell(t, _) => *t,
        }
    }
}

impl Display for RawVc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RawVc::TaskOutput(task) => {
                write!(f, "output of {}", task)
            }
            RawVc::TaskCell(task, index) => {
                write!(f, "value {} of {}", index, task)
            }
        }
    }
}

pub struct ReadRawVcFuture<T: Any + Send + Sync> {
    turbo_tasks: Arc<dyn TurboTasksApi>,
    strongly_consistent: bool,
    current: RawVc,
    listener: Option<EventListener>,
    phantom_data: PhantomData<Pin<Box<T>>>,
}

impl<T: Any + Send + Sync> ReadRawVcFuture<T> {
    fn new(vc: RawVc) -> Self {
        let tt = turbo_tasks();
        tt.notify_scheduled_tasks();
        ReadRawVcFuture {
            turbo_tasks: tt,
            strongly_consistent: false,
            current: vc,
            listener: None,
            phantom_data: PhantomData,
        }
    }

    fn new_strongly_consistent(vc: RawVc) -> Self {
        let tt = turbo_tasks();
        tt.notify_scheduled_tasks();
        ReadRawVcFuture {
            turbo_tasks: tt,
            strongly_consistent: true,
            current: vc,
            listener: None,
            phantom_data: PhantomData,
        }
    }

    pub fn map<O, F: Fn(&T) -> &O>(self, func: F) -> ReadAndMapRawVcFuture<T, O, F> {
        ReadAndMapRawVcFuture {
            inner: self,
            func: Some(func),
        }
    }
}

impl<T: Any + Send + Sync> Future for ReadRawVcFuture<T> {
    type Output = Result<RawVcReadResult<T>>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        self.turbo_tasks.notify_scheduled_tasks();
        let this = self.get_mut();
        'outer: loop {
            if let Some(listener) = &mut this.listener {
                let listener = unsafe { Pin::new_unchecked(listener) };
                if listener.poll(cx).is_pending() {
                    return Poll::Pending;
                }
                this.listener = None;
            }
            let mut listener = match this.current {
                RawVc::TaskOutput(task) => match this
                    .turbo_tasks
                    .try_read_task_output(task, this.strongly_consistent)
                {
                    Ok(Ok(vc)) => {
                        this.strongly_consistent = false;
                        this.current = vc;
                        continue 'outer;
                    }
                    Ok(Err(listener)) => listener,
                    Err(err) => return Poll::Ready(Err(err)),
                },
                RawVc::TaskCell(task, index) => {
                    match this.turbo_tasks.try_read_task_cell(task, index) {
                        Ok(Ok(content)) => {
                            return Poll::Ready(content.cast::<T>());
                        }
                        Ok(Err(listener)) => listener,
                        Err(err) => return Poll::Ready(Err(err)),
                    }
                }
            };
            match Pin::new(&mut listener).poll(cx) {
                Poll::Ready(_) => continue,
                Poll::Pending => {
                    this.listener = Some(listener);
                    return Poll::Pending;
                }
            };
        }
    }
}

pub struct ReadAndMapRawVcFuture<T: Any + Send + Sync, O, F: Fn(&T) -> &O> {
    inner: ReadRawVcFuture<T>,
    func: Option<F>,
}

impl<T: Any + Send + Sync, O, F: Fn(&T) -> &O> Future for ReadAndMapRawVcFuture<T, O, F> {
    type Output = Result<RawVcReadAndMapResult<T, O, F>>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let inner_pin = Pin::new(&mut this.inner);
        match inner_pin.poll(cx) {
            Poll::Ready(r) => Poll::Ready(match r {
                Ok(r) => Ok(r.map_read_result(this.func.take().unwrap())),
                Err(e) => Err(e),
            }),
            Poll::Pending => Poll::Pending,
        }
    }
}