use bearings::{class, interface, object, Result, Service};

use tokio::io::duplex;

#[interface(())]
trait Performer {
    async fn perform(&self, string: String) -> String;
}

#[derive(Default)]
struct Reverser;

#[class(())]
impl Performer for Reverser {
    async fn perform(&self, string: String) -> String {
        Ok(string.chars().rev().collect())
    }
}

#[object(())]
struct SimpleExample {
    performer: Performer,
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    let (server, client) = duplex(1024);

    let server = Service::new(server);
    let client = Service::new(client);

    let server_controller = server.controller();
    let client_controller = client.controller();

    tokio::spawn(async move {
        server.run().await.unwrap();
    });
    tokio::spawn(async move {
        client.run().await.unwrap();
    });

    server_controller
        .add_object(SimpleExample::new(Reverser::default()))
        .await?;

    let client = client_controller.connect::<SimpleExample>().await?;

    let string = "abcdef";
    let reversed = client
        .performer
        .lock()
        .await
        .perform(String::from(string))
        .await?;

    assert_eq!(string.chars().rev().collect::<String>(), reversed);

    Ok(())
}
