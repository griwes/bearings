use bearings::{async_trait, Service};
use bearings_proc::{class, object};

use tokio::io::duplex;

#[class]
trait Performer {
    async fn perform(&self, string: String) -> Result<String, Box<dyn std::error::Error>>;
}

#[derive(Default)]
struct Reverser;

#[async_trait]
impl Performer for Reverser {
    async fn perform(&self, string: String) -> Result<String, Box<dyn std::error::Error>> {
        Ok(string.chars().rev().collect())
    }
}

#[object]
struct SimpleExample {
    performer: Performer,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
