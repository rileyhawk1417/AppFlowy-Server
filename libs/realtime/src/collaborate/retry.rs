use crate::collaborate::CollabClientStream;
use anyhow::{anyhow, Error};
use collab::core::origin::CollabOrigin;
use database::collab::CollabStorage;
use futures_util::SinkExt;
use parking_lot::Mutex;
use realtime_entity::collab_msg::CollabMessage;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::future;
use std::future::Future;
use std::iter::Take;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::RwLock;

use crate::entities::{ClientMessage, Editing, RealtimeUser};
use tokio_retry::strategy::FixedInterval;
use tokio_retry::{Action, Condition, Retry, RetryIf};

use crate::collaborate::group::CollabGroupCache;
use crate::collaborate::permission::CollabAccessControl;
use crate::error::RealtimeError;
use tracing::{error, trace, warn};

pub(crate) struct SubscribeGroupIfNeed<'a, U, S, P> {
  pub(crate) client_msg: &'a ClientMessage<U>,
  pub(crate) groups: &'a Arc<CollabGroupCache<S, U>>,
  pub(crate) edit_collab_by_user: &'a Arc<Mutex<HashMap<U, HashSet<Editing>>>>,
  pub(crate) client_stream_by_user: &'a Arc<RwLock<HashMap<U, CollabClientStream>>>,
  pub(crate) access_control: &'a Arc<P>,
}

impl<'a, U, S, P> SubscribeGroupIfNeed<'a, U, S, P>
where
  U: RealtimeUser,
  S: CollabStorage,
  P: CollabAccessControl,
{
  pub(crate) fn run(
    self,
  ) -> RetryIf<Take<FixedInterval>, SubscribeGroupIfNeed<'a, U, S, P>, SubscribeGroupCondition<U>>
  {
    let weak_client_stream = Arc::downgrade(self.client_stream_by_user);
    let retry_strategy = FixedInterval::new(Duration::from_secs(2)).take(5);
    RetryIf::spawn(
      retry_strategy,
      self,
      SubscribeGroupCondition(weak_client_stream),
    )
  }
}

impl<'a, U, S, P> Action for SubscribeGroupIfNeed<'a, U, S, P>
where
  U: RealtimeUser,
  S: CollabStorage,
  P: CollabAccessControl,
{
  type Future = Pin<Box<dyn Future<Output = Result<Self::Item, Self::Error>> + 'a>>;
  type Item = ();
  type Error = RealtimeError;

  fn run(&mut self) -> Self::Future {
    Box::pin(async {
      let object_id = self.client_msg.content.object_id();
      if !self.groups.contains_group(object_id).await? {
        // When create a group, the message must be the init sync message.
        match &self.client_msg.content {
          CollabMessage::ClientInit(client_init) => {
            let uid = client_init
              .origin
              .client_user_id()
              .ok_or(RealtimeError::UnexpectedData("The client user id is empty"))?;

            self
              .groups
              .create_group(
                uid,
                &client_init.workspace_id,
                object_id,
                client_init.collab_type.clone(),
              )
              .await;
          },
          _ => {
            return Err(RealtimeError::UnexpectedData(
              "The first message must be init sync message",
            ));
          },
        }
      }

      // If the client's stream is already subscribe to the collab, return.
      if self
        .groups
        .contains_user(object_id, &self.client_msg.user)
        .await
        .unwrap_or(false)
      {
        return Ok(());
      }

      let origin = match self.client_msg.content.origin() {
        None => {
          error!("🔴The origin from client message is empty");
          &CollabOrigin::Empty
        },
        Some(client) => client,
      };
      match self
        .client_stream_by_user
        .write()
        .await
        .get_mut(&self.client_msg.user)
      {
        None => warn!("The client stream is not found"),
        Some(client_stream) => {
          if let Some(collab_group) = self.groups.get_group(object_id).await {
            if let Entry::Vacant(entry) = collab_group
              .subscribers
              .write()
              .await
              .entry(self.client_msg.user.clone())
            {
              trace!(
                "[💭Server]: {} subscribe group:{}",
                self.client_msg.user,
                self.client_msg.content.object_id()
              );

              let client_uid = self.client_msg.user.uid();
              self
                .edit_collab_by_user
                .lock()
                .entry(self.client_msg.user.clone())
                .or_default()
                .insert(Editing {
                  object_id: object_id.to_string(),
                  origin: origin.clone(),
                });

              let sink_permission_service = self.access_control.clone();
              let stream_permission_service = self.access_control.clone();

              let (sink, stream) = client_stream.client_channel::<CollabMessage, _, _>(
                object_id,
                move |object_id, msg| {
                  if msg.object_id() != object_id {
                    return Box::pin(future::ready(false));
                  }

                  let object_id = object_id.to_string();
                  let cloned_sink_permission_service = sink_permission_service.clone();
                  Box::pin(async move {
                    match cloned_sink_permission_service
                      .can_receive_collab_update(&client_uid, &object_id)
                      .await
                    {
                      Ok(is_allowed) => {
                        if !is_allowed {
                          warn!(
                            "user:{} is not allowed to receive object:{} updates",
                            client_uid, object_id,
                          );
                        }

                        is_allowed
                      },
                      Err(err) => {
                        trace!(
                          "user:{} fail to receive updates by error: {}",
                          client_uid,
                          err
                        );
                        false
                      },
                    }
                  })
                },
                move |object_id, msg| {
                  if msg.object_id() != object_id {
                    return Box::pin(future::ready(false));
                  }

                  let is_init = msg.is_init();
                  let object_id = object_id.to_string();
                  let cloned_stream_permission_service = stream_permission_service.clone();

                  Box::pin(async move {
                    // If the message is init sync, and it's allow the send to the group.
                    if is_init {
                      return true;
                    }

                    match cloned_stream_permission_service
                      .can_send_collab_update(&client_uid, &object_id)
                      .await
                    {
                      Ok(is_allowed) => {
                        if !is_allowed {
                          warn!(
                            "client:{} is not allowed to send object:{} updates",
                            client_uid, object_id,
                          );
                        }
                        is_allowed
                      },
                      Err(err) => {
                        trace!(
                          "client:{} can't  send update with error: {}",
                          client_uid,
                          err
                        );
                        false
                      },
                    }
                  })
                },
              );

              entry.insert(
                collab_group
                  .broadcast
                  .subscribe(origin.clone(), sink, stream),
              );
            }
          }
        },
      }

      Ok(())
    })
  }
}

pub struct SubscribeGroupCondition<U>(pub Weak<RwLock<HashMap<U, CollabClientStream>>>);
impl<U> Condition<RealtimeError> for SubscribeGroupCondition<U> {
  fn should_retry(&mut self, _error: &RealtimeError) -> bool {
    self.0.upgrade().is_some()
  }
}

pub struct SinkCollabMessageAction<'a, Sink> {
  pub sink: &'a Arc<tokio::sync::Mutex<Sink>>,
  pub message: CollabMessage,
}

impl<'a, Sink> SinkCollabMessageAction<'a, Sink>
where
  Sink: SinkExt<CollabMessage> + Send + Sync + Unpin + 'a,
{
  pub fn run(self) -> Retry<Take<FixedInterval>, SinkCollabMessageAction<'a, Sink>> {
    let retry_strategy = FixedInterval::new(Duration::from_secs(2)).take(5);
    Retry::spawn(retry_strategy, self)
  }
}

impl<'a, Sink> Action for SinkCollabMessageAction<'a, Sink>
where
  Sink: SinkExt<CollabMessage> + Send + Sync + Unpin + 'a,
{
  type Future = Pin<Box<dyn Future<Output = Result<Self::Item, Self::Error>> + Send + Sync + 'a>>;
  type Item = ();
  type Error = RealtimeError;

  fn run(&mut self) -> Self::Future {
    let sink = self.sink.clone();
    let message = self.message.clone();
    Box::pin(async move {
      let mut sink = sink
        .try_lock()
        .map_err(|err| RealtimeError::Internal(Error::from(err)))?;
      sink
        .send(message)
        .await
        .map_err(|_err| RealtimeError::Internal(anyhow!("Sink message fail")))?;
      Ok(())
    })
  }
}
