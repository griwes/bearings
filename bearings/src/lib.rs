extern crate proc_macro;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

use uuid::Uuid;

use std::collections::HashMap;
use std::sync::Arc;

pub use async_trait::async_trait;

pub type StatePtr<T> = Arc<Mutex<ServiceState<T>>>;

pub trait Object {
    fn uuid(&self) -> Uuid;
}

pub trait ObjectClient {
    fn build<T>(state: StatePtr<T>) -> Self;
}

pub struct ServiceState<T> {
    rw: T,
    objects: HashMap<Uuid, Box<dyn Object>>,
}

impl<T> ServiceState<T> {
    pub fn new(conn: T) -> Self {
        return Self {
            rw: conn,
            objects: <_>::default(),
        };
    }
}

pub struct Service<T: AsyncBufReadExt + AsyncWriteExt> {
    state: StatePtr<T>,
}

impl<T: AsyncBufReadExt + AsyncWriteExt> Service<T> {
    pub fn new(conn: T) -> Self {
        Self {
            state: Arc::new(Mutex::from(ServiceState::new(conn))),
        }
    }

    pub fn controller(&self) -> Controller<T> {
        Controller {
            state: self.state.clone(),
        }
    }

    pub async fn run(self) {
        unimplemented!();
    }
}

unsafe impl<T: AsyncBufReadExt + AsyncWriteExt> Send for Service<T> {}

pub struct Controller<T> {
    state: StatePtr<T>,
}

impl<T> Controller<T> {
    pub async fn add_object<U: 'static + Object>(
        &self,
        object: U,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.state
            .lock()
            .await
            .objects
            .insert(object.uuid(), Box::from(object));

        Ok(())
    }

    pub async fn connect<U: Object + ObjectClient>(&self) -> Result<U, Box<dyn std::error::Error>> {
        Ok(U::build(self.state.clone()))
    }
}
