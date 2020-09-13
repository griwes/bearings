#[cfg(test)]
mod basic {
    #[bearings::interface(usize)]
    trait Interface {
        async fn run(&mut self, val: usize) -> usize;
        async fn fail(&mut self, val: usize) -> ();
    }

    #[derive(Default)]
    struct InterfaceTester {
        value: std::sync::Arc<tokio::sync::Mutex<usize>>,
    }

    impl InterfaceTester {
        fn new(counter: std::sync::Arc<tokio::sync::Mutex<usize>>) -> Self {
            Self { value: counter }
        }
    }

    #[bearings::class(usize)]
    impl Interface for InterfaceTester {
        async fn run(&mut self, val: usize) -> usize {
            let mut lock = self.value.lock().await;
            let result = *lock;
            *lock = val;
            Ok(result)
        }

        async fn fail(&mut self, val: usize) -> () {
            let mut lock = self.value.lock().await;
            let result = *lock;
            *lock = val;
            Err(bearings::as_error(result))
        }
    }

    #[bearings::object(usize)]
    struct Tester {
        interface: Interface,
    }

    #[bearings::object(usize)]
    struct Fake {
        interface: Interface,
    }

    #[bearings::object(usize)]
    struct Invalid {}

    #[tokio::test]
    async fn basic() {
        let (server, client) = tokio::io::duplex(1024);

        let server = bearings::Service::new(server);
        let client = bearings::Service::new(client);

        let server_controller = server.controller();
        let client_controller = client.controller();

        tokio::spawn(async move {
            server.run().await.unwrap();
        });
        tokio::spawn(async move {
            client.run().await.unwrap();
        });

        let counter = std::sync::Arc::new(tokio::sync::Mutex::new(17));

        assert!(server_controller
            .add_object(Tester::new(InterfaceTester::new(counter.clone())))
            .await
            .is_ok());

        assert!(server_controller
            .add_object(Tester::new(InterfaceTester::new(counter.clone())))
            .await
            .is_err());

        let client_result = client_controller.connect::<Tester>().await;
        assert!(client_result.is_ok());

        // TODO: implement connect validation
        // assert!(client_controller.connect::<Fake>().await.is_err());
        // assert!(client_controller.connect::<Invalid>().await.is_err());

        let client = client_result.unwrap();

        let result = client.interface.lock().await.run(123).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 17);
        assert_eq!(*counter.lock().await, 123);

        let result = client.interface.lock().await.fail(1717).await;

        assert!(result.is_err());
        assert_eq!(*result.err().unwrap().to_user().unwrap(), 123);
        assert_eq!(*counter.lock().await, 1717);
    }
}
