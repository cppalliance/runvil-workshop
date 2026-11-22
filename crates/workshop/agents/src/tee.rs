//! The recorder a conversation's run writes through: every call goes on
//! to the inner recorder, and what the inner recorder accepted also
//! reaches the conversation.
//!
//! The tee notes the run the inner recorder begins. The Harness hands it
//! each event with the event's record. The tee passes both on to the
//! inner recorder, and it hands the event to the conversation once the
//! inner recorder has accepted the record, so the record, the live
//! broadcast, and the transcript agree event for event. A refused write
//! is returned as the inner recorder's own, and its event reaches no one.

use std::sync::Arc;

use runvil::event::Event;
use runvil::harness::{Record, RunId, RunMeta, RunRecorder};
use runvil::{BoxFuture, Error};

use crate::conversation::Conversation;

/// A [`RunRecorder`] over an inner recorder that feeds one conversation.
pub(crate) struct ConversationRecorder {
    conversation: Conversation,
    inner: Arc<dyn RunRecorder>,
}

impl ConversationRecorder {
    /// The tee over `inner` feeding `conversation`.
    pub(crate) fn new(conversation: Conversation, inner: Arc<dyn RunRecorder>) -> Self {
        Self {
            conversation,
            inner,
        }
    }
}

impl RunRecorder for ConversationRecorder {
    fn begin_run(&self, meta: RunMeta) -> BoxFuture<'_, Result<RunId, Error>> {
        Box::pin(async move {
            let run = self.inner.begin_run(meta).await?;
            self.conversation.note_run(run);
            Ok(run)
        })
    }

    fn append(&self, run: RunId, record: Record) -> BoxFuture<'_, Result<(), Error>> {
        self.inner.append(run, record)
    }

    fn append_event<'a>(
        &'a self,
        run: RunId,
        record: Record,
        event: &'a Event,
    ) -> BoxFuture<'a, Result<(), Error>> {
        Box::pin(async move {
            self.inner.append_event(run, record, event).await?;
            self.conversation.observe(event);
            Ok(())
        })
    }

    fn end_run(
        &self,
        run: RunId,
        result: Result<String, Error>,
    ) -> BoxFuture<'_, Result<(), Error>> {
        self.inner.end_run(run, result)
    }
}

#[cfg(test)]
#[path = "tee-tests.rs"]
mod tests;
