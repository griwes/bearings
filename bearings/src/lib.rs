extern crate proc_macro;

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

pub type StatePtr<T> = Arc<ServiceState<T>>;

#[async_trait]
pub trait Object {
    fn uuid() -> Uuid
    where
        Self: Sized;

    async fn invoke(
        &self,
        call: FunctionCall<serde_json::value::Value>,
    ) -> Result<Message<()>, Box<dyn std::error::Error>>;
}

pub trait ObjectClient<'a> {
    fn build<T: 'a + Send + Unpin + AsyncRead + AsyncWrite>(state: StatePtr<T>) -> Self;
}

#[derive(Debug)]
pub enum Awaiter {
    Empty,
    Waker(Waker),
    Return(ReturnValue<serde_json::value::Value>),
}

pub struct ServiceState<T> {
    pub r: Mutex<BufReader<ReadHalf<T>>>,
    pub w: Mutex<BufWriter<WriteHalf<T>>>,
    pub id: Mutex<u64>,
    objects: Mutex<HashMap<Uuid, Mutex<Box<dyn Object + Send>>>>,
    pub awaiters: Mutex<HashMap<u64, Mutex<Awaiter>>>,
}

impl<T: AsyncRead + AsyncWrite> ServiceState<T> {
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

pub struct Service<T: AsyncRead + AsyncWrite> {
    state: StatePtr<T>,
}

async fn launch<T: AsyncRead + AsyncWrite>(
    state: StatePtr<T>,
    call: FunctionCall<serde_json::value::Value>,
) -> () {
    let mut objects = state.objects.lock().await;
    println!("{:?}", objects.keys());

    match objects.get_mut(&call.uuid) {
        Some(object) => {
            let result = object.lock().await;
            let result = result.invoke(call);
            let result = result.await.unwrap();
            let result = serde_json::to_string(&result).unwrap();

            let mut w = state.w.lock().await;
            w.write_all(format!("{}\0", result).as_bytes())
                .await
                .unwrap();
            w.flush().await.unwrap();
        }
        None => panic!("attempted to invoke a function of an object that is not registered"),
    }
}

impl<T: 'static + AsyncRead + AsyncWrite + Unpin + Send> Service<T> {
    pub fn new(conn: T) -> Self {
        Self {
            state: Arc::new(ServiceState::new(conn)),
        }
    }

    pub fn controller(&self) -> Controller<T> {
        Controller {
            state: self.state.clone(),
        }
    }

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let mut buf = Vec::new();
        let state = self.state.clone();

        loop {
            state.r.lock().await.read_until(b'\0', &mut buf).await?;
            buf.pop();
            let message: Message<serde_json::value::Value> = serde_json::from_reader(&*buf)?;
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
                    let awaiter = awaiter.lock().await;

                    map.insert(ret.id, Mutex::from(Awaiter::Return(ret)));

                    match &*awaiter {
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

unsafe impl<T: AsyncRead + AsyncWrite> Send for Service<T> {}

pub struct Controller<T: Send> {
    state: StatePtr<T>,
}

impl<'a, T: 'a + Send + Unpin + AsyncRead + AsyncWrite> Controller<T> {
    pub async fn add_object<U: Object + Send + 'static>(
        &self,
        object: U,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.state.objects.lock().await.insert(
            U::uuid(),
            Mutex::from(Box::new(object) as Box<dyn Object + Send>),
        );

        Ok(())
    }

    pub async fn connect<U: Object + ObjectClient<'a>>(
        &self,
    ) -> Result<U, Box<dyn std::error::Error>> {
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
pub enum Message<T: Serialize> {
    Call(FunctionCall<T>),
    Return(ReturnValue<serde_json::value::Value>),
}

pub struct ReplyFuture<T: serde::de::DeserializeOwned, U> {
    state: StatePtr<U>,
    request_id: u64,
    __: PhantomData<T>,
}

unsafe impl<T: serde::de::DeserializeOwned, U> Send for ReplyFuture<T, U> {}

impl<T: serde::de::DeserializeOwned, U> ReplyFuture<T, U> {
    pub fn new(state: StatePtr<U>, request_id: u64) -> Self {
        ReplyFuture {
            state: state,
            request_id: request_id,
            __: <_>::default(),
        }
    }
}

impl<T: serde::de::DeserializeOwned, U> Future for ReplyFuture<T, U> {
    type Output = Result<T, Box<dyn std::error::Error>>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        loop {
            match self.state.awaiters.try_lock() {
                Result::Ok(mut map) => match map.remove(&self.request_id) {
                    Option::Some(awaiter) => match awaiter.try_lock() {
                        Result::Ok(awaiter) => match &*awaiter {
                            Awaiter::Return(ret) => {
                                println!("{}", ret.result);
                                let parsed: Result<T, _> =
                                    serde_json::from_value(ret.result.clone());
                                match parsed {
                                    Result::Ok(value) => return Poll::Ready(Ok(value)),
                                    Result::Err(err) => {
                                        println!("{}", err);
                                        panic!("failed to parse the result in the response")
                                    }
                                }
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
                        Result::Err(_) => panic!("how did this happen?"),
                    },

                    Option::None => panic!("tried to await on a call result of a not issued call"),
                },

                Result::Err(_) => continue,
            }
        }
    }
}
