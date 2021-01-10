use dotenv::dotenv;
use log::{debug, warn};
use std::sync::Arc;

use tokio::sync::mpsc;
use tonic::transport::Server;
use tonic::{Request, Response, Status};

mod api;
mod db;

use crate::db::DataSources;
use hello_world::hello_world_service_server::{HelloWorldService, HelloWorldServiceServer};
use hello_world::{
    DeleteHelloWorldRequest, GetHelloWorldRequest, HelloWorld, ListHelloWorldsRequest,
    UpdateHelloWorldRequest,
};

pub mod hello_world {
    tonic::include_proto!("cosm.hello_world");
}

#[derive(Clone)]
pub struct HelloWorlds {
    data_sources: Arc<DataSources>,
}

#[tonic::async_trait]
impl HelloWorldService for HelloWorlds {
    async fn create_hello_world(
        &self,
        request: Request<HelloWorld>,
    ) -> Result<Response<HelloWorld>, tonic::Status> {
        let item = request.into_inner();
        api::items::create_one(&self.data_sources.hello_worlds, item).await
    }

    async fn get_hello_world(
        &self,
        request: Request<GetHelloWorldRequest>,
    ) -> Result<tonic::Response<HelloWorld>, tonic::Status> {
        warn!("GetHelloWorld = {:?}", request);

        api::items::get_by_id(&self.data_sources.hello_worlds, &request.get_ref().id).await
    }

    type ListHelloWorldsStream = mpsc::Receiver<Result<HelloWorld, Status>>;
    async fn list_hello_worlds(
        &self,
        request: Request<ListHelloWorldsRequest>,
    ) -> Result<tonic::Response<Self::ListHelloWorldsStream>, tonic::Status> {
        debug!("ListHelloWorlds = {:?}", request);

        let request = request.get_ref();
        if request.limit > 100 {
            return Err(Status::invalid_argument("Maximum number of items is 100"));
        }

        return Ok(api::items::stream(&self.data_sources.hello_worlds, request).await);
    }

    async fn update_hello_world(
        &self,
        request: tonic::Request<UpdateHelloWorldRequest>,
    ) -> Result<tonic::Response<HelloWorld>, tonic::Status> {
        warn!("UpdateHelloWorld = {:?}", request);

        let request = request.get_ref();
        api::items::update_one(&self.data_sources.hello_worlds, request).await
    }

    async fn delete_hello_world(
        &self,
        request: tonic::Request<DeleteHelloWorldRequest>,
    ) -> Result<tonic::Response<()>, tonic::Status> {
        warn!("DeleteHelloWorld = {:?}", request);

        api::items::delete_by_id(&self.data_sources.hello_worlds, &request.get_ref().id).await
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    env_logger::init();
    let addr = "0.0.0.0:10000".parse().unwrap();

    warn!("HelloWorldService listening on: {}", addr);
    let hello_worlds = HelloWorlds {
        data_sources: db::connect().await,
    };

    let svc = HelloWorldServiceServer::new(hello_worlds);

    Server::builder().add_service(svc).serve(addr).await?;

    Ok(())
}
