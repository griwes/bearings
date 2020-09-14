extern crate proc_macro;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use tokio::io::{
    AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, BufWriter, ReadHalf,
    WriteHalf,
};
use tokio::sync::Mutex;

use uuid::Uuid;

use std::collections::HashMap;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

pub use async_trait::async_trait;
pub use bearings_proc::{class, interface, object};

pub type StatePtr<T, E> = Arc<ServiceState<T, E>>;

mod error;
use error::Error::*;
pub use error::*;

#[async_trait]
pub trait Object<E: std::fmt::Debug + Serialize> {
    fn uuid() -> Uuid
    where
        Self: Sized;

    async fn invoke(
        &self,
        call: FunctionCall<serde_json::value::Value>,
    ) -> Result<Message<(), E>, E>;
}

pub trait ObjectClient<'a, E: std::fmt::Debug + Serialize> {
    fn build<T: 'a + Send + Unpin + AsyncRead + AsyncWrite>(state: StatePtr<T, E>) -> Self;
}

#[derive(Debug)]
pub enum Awaiter<E: std::fmt::Debug + Serialize> {
    Empty,
    Waker(Waker),
    Return(ReturnValue<serde_json::value::Value>),
    Error(ErrorResult<E>),
}

pub struct ServiceState<T, E: std::fmt::Debug + Serialize> {
    pub r: Mutex<BufReader<ReadHalf<T>>>,
    pub w: Mutex<BufWriter<WriteHalf<T>>>,
    pub id: Mutex<u64>,
    objects: Mutex<HashMap<Uuid, Mutex<Box<dyn Object<E> + Send>>>>,
    pub awaiters: Mutex<HashMap<u64, Mutex<Awaiter<E>>>>,
}

impl<T: AsyncRead + AsyncWrite, E: std::fmt::Debug + Serialize> ServiceState<T, E> {
    pub fn new(conn: T) -> Self {
        let (r, w) = tokio::io::split(conn);
        return Self {
            r: Mutex::from(BufReader::new(r)),
            w: Mutex::from(BufWriter::new(w)),
            id: <_>::default(),
            objects: <_>::default(),
            awaiters: <_>::default(),
        };
    }
}

pub struct Service<T: AsyncRead + AsyncWrite, E: std::fmt::Debug + Serialize> {
    state: StatePtr<T, E>,
}

async fn launch<T: AsyncRead + AsyncWrite, E: std::fmt::Debug + Serialize>(
    state: StatePtr<T, E>,
    call: FunctionCall<serde_json::value::Value>,
) -> () {
    let mut objects = state.objects.lock().await;
    println!("{:?}", objects.keys());

    let id = call.id;

    let result = match objects.get_mut(&call.uuid) {
        Some(object) => {
            let result = object.lock().await;
            let result = result.invoke(call);
            result.await
        }
        None => Err(UnknownObject(call.uuid)),
    };

    let result = match result {
        Ok(message) => serde_json::to_string(&message).unwrap(),
        Err(error) => serde_json::to_string(&Message::<(), E>::Error(ErrorResult {
            id: id,
            error: error,
        }))
        .unwrap(),
    };

    let mut w = state.w.lock().await;
    w.write_all(format!("{}\0", result).as_bytes())
        .await
        .unwrap();
    w.flush().await.unwrap();
}

impl<
        T: 'static + AsyncRead + AsyncWrite + Unpin + Send,
        E: 'static + std::fmt::Debug + Serialize + DeserializeOwned + Send,
    > Service<T, E>
{
    pub fn new(conn: T) -> Self {
        Self {
            state: Arc::new(ServiceState::new(conn)),
        }
    }

    pub fn controller(&self) -> Controller<T, E> {
        Controller {
            state: self.state.clone(),
        }
    }

    pub async fn run(self) -> Result<(), E> {
        let mut buf = Vec::new();
        let state = self.state.clone();

        loop {
            buf.clear();
            state.r.lock().await.read_until(b'\0', &mut buf).await?;
            buf.pop();
            println!("{:?}", std::str::from_utf8(&*buf));
            let message: Message<serde_json::value::Value, E> = serde_json::from_reader(&*buf)?;
            println!("{:?}", message);

            match message {
                Message::Return(ret) => {
                    let mut map = self.state.awaiters.lock().await;
                    println!("{:?}", map);
                    let awaiter = match map.remove(&ret.id) {
                        Option::None => panic!(
                            "received a reply to a call that was not issued, or one that has already been replied to"
                        ),
                        Option::Some(awaiter) => awaiter,
                    };
                    let awaiter = awaiter.into_inner();

                    map.insert(ret.id, Mutex::from(Awaiter::Return(ret)));

                    match awaiter {
                        Awaiter::Waker(waker) => {
                            waker.wake_by_ref();
                        }
                        _ => (),
                    }
                }
                Message::Error(err) => {
                    let mut map = self.state.awaiters.lock().await;
                    println!("{:?}", map);
                    let awaiter = match map.remove(&err.id) {
                        Option::None => panic!(
                            "received a reply to a call that was not issued, or one that has already been replied to"
                        ),
                        Option::Some(awaiter) => awaiter,
                    };
                    let awaiter = awaiter.into_inner();

                    map.insert(err.id, Mutex::from(Awaiter::Error(err)));

                    match awaiter {
                        Awaiter::Waker(waker) => {
                            waker.wake_by_ref();
                        }
                        _ => (),
                    }
                }
                Message::Call(call) => {
                    let state = self.state.clone();
                    tokio::spawn(launch(state, call));
                }
            }
        }
    }
}

unsafe impl<T: AsyncRead + AsyncWrite, E: std::fmt::Debug + Serialize> Send for Service<T, E> {}

pub struct Controller<T: Send, E: std::fmt::Debug + Serialize> {
    state: StatePtr<T, E>,
}

impl<'a, T: 'a + Send + Unpin + AsyncRead + AsyncWrite, E: std::fmt::Debug + Serialize>
    Controller<T, E>
{
    pub async fn add_object<U: Object<E> + Send + 'static>(&self, object: U) -> Result<(), E> {
        let uuid = U::uuid();
        let mut objects = self.state.objects.lock().await;

        if objects.contains_key(&uuid) {
            return Err(UuidAlreadyInUse);
        }

        objects.insert(
            uuid,
            Mutex::from(Box::new(object) as Box<dyn Object<E> + Send>),
        );

        Ok(())
    }

    pub async fn connect<U: Object<E> + ObjectClient<'a, E>>(&self) -> Result<U, E> {
        Ok(U::build(self.state.clone()))
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FunctionCall<T: Serialize> {
    pub id: u64,
    pub uuid: Uuid,
    pub member: String,
    pub method: String,
    pub arguments: T,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReturnValue<T: Serialize> {
    pub id: u64,
    pub result: T,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ErrorResult<E: std::fmt::Debug + Serialize> {
    pub id: u64,
    pub error: Error<E>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Message<T: Serialize, E: std::fmt::Debug + Serialize> {
    Call(FunctionCall<T>),
    Return(ReturnValue<serde_json::value::Value>),
    Error(ErrorResult<E>),
}

pub struct ReplyFuture<T: DeserializeOwned, U, E: std::fmt::Debug + Serialize> {
    state: StatePtr<U, E>,
    request_id: u64,
    __: PhantomData<T>,
}

unsafe impl<T: DeserializeOwned, U, E: std::fmt::Debug + Serialize> Send for ReplyFuture<T, U, E> {}

impl<T: DeserializeOwned, U, E: std::fmt::Debug + Serialize> ReplyFuture<T, U, E> {
    pub fn new(state: StatePtr<U, E>, request_id: u64) -> Self {
        ReplyFuture {
            state: state,
            request_id: request_id,
            __: <_>::default(),
        }
    }
}

impl<T: DeserializeOwned, U, E: std::fmt::Debug + Serialize> Future for ReplyFuture<T, U, E> {
    type Output = Result<T, E>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        loop {
            match self.state.awaiters.try_lock() {
                Ok(mut map) => match map.remove(&self.request_id) {
                    Some(awaiter) => match awaiter.into_inner() {
                        Awaiter::Return(ret) => {
                            println!("{:?}", ret.result);
                            let parsed: std::result::Result<T, _> =
                                serde_json::from_value(ret.result.clone());
                            match parsed {
                                Ok(value) => return Poll::Ready(Ok(value)),
                                Err(err) => return Poll::Ready(Err(Error::from(err))),
                            }
                        }

                        Awaiter::Error(err) => {
                            println!("{:?}", err.error);
                            return Poll::Ready(Err(err.error));
                        }

                        _ => {
                            map.insert(
                                self.request_id,
                                Mutex::from(Awaiter::Waker(ctx.waker().clone())),
                            );
                            println!("{:?}", map);
                            return Poll::Pending;
                        }
                    },

                    None => panic!("tried to await on a call result of a not issued call"),
                },

                Err(_) => continue,
            }
        }
    }
}
