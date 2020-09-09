use bearings::{async_trait, Service};
use bearings_proc::{class, method, object};

use tokio::io::{duplex, BufStream};

#[class]
trait Performer {
    #[method]
    async fn perform(&self, string: String) -> String;
}

#[derive(Default)]
struct Reverser;

#[async_trait]
impl Performer for Reverser {
    async fn perform(&self, string: String) -> String {
        string.chars().rev().collect()
    }
}

#[object]
struct SimpleExample {
    performer: Box<dyn Performer>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (server, client) = duplex(1024);

    let server = BufStream::new(server);
    let client = BufStream::new(client);

    let server = Service::new(server);
    let client = Service::new(client);

    let server_controller = server.controller();
    let client_controller = client.controller();

    tokio::spawn(async move {
        server.run().await;
    });
    tokio::spawn(async move {
        client.run().await;
    });

    server_controller
        .add_object(SimpleExample::new(Box::new(Reverser::default())))
        .await?;

    let client = client_controller.connect::<SimpleExample>().await?;

    let string = "abcdef";
    let reversed = client.performer.perform(String::from(string)).await;

    assert_eq!(string, reversed);

    Ok(())
}

