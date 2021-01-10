use futures::future;
use futures::stream::StreamExt;
use log::{debug, error, warn};
use mongodb::{
    bson::{doc, from_bson, to_bson, Bson, Document},
    options::FindOptions,
    Collection,
};
use tokio::sync::mpsc;
use tonic::{Response, Status};

use crate::api::get_timestamp;
use crate::db::id::{with_bson, ID};
use crate::hello_world::{HelloWorld, ListHelloWorldsRequest, UpdateHelloWorldRequest};

pub async fn create_one(
    collection: &Collection,
    mut item: HelloWorld,
) -> Result<Response<HelloWorld>, tonic::Status> {
    if item.name == "" {
        return Err(Status::invalid_argument("name_required"));
    }
    item.created_at = Some(get_timestamp());

    // create in db
    let serialized_member = to_bson(&item).map_err(|e| Status::unavailable(e.to_string()))?;
    if let Bson::Document(mut document) = serialized_member {
        // remove the id of this object so that mongo will generate
        document.remove("_id");
        let insert_result = collection
            .insert_one(document, None)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        // convert id to a string
        item.id = with_bson(&insert_result.inserted_id);
        Ok(Response::new(item))
    } else {
        Err(Status::internal("INTERNAL ERROR"))
    }
}

pub async fn get_by_id(
    collection: &Collection,
    id: &str,
) -> Result<tonic::Response<HelloWorld>, tonic::Status> {
    let id = ID::from_string(id)?;
    let filter = doc! { "_id": id.to_bson() };
    let some_item = collection.find_one(filter, None).await.map_err(|_| {
        error!("an error occurred");
        Status::internal("DATABASE ERROR")
    })?;

    match some_item {
        Some(doc) => {
            debug!("item: {:?}", doc);
            let item: HelloWorld =
                from_bson(Bson::Document(doc)).map_err(|e| Status::internal(e.to_string()))?;
            Ok(Response::new(item))
        }
        None => Err(Status::not_found("NOT FOUND")),
    }
}

pub async fn stream(
    collection: &Collection,
    request: &ListHelloWorldsRequest,
) -> Response<mpsc::Receiver<Result<HelloWorld, Status>>> {
    let (mut tx, rx) = mpsc::channel(100);

    let options = FindOptions::builder()
        .skip(Some(request.start as i64))
        .limit(Some(request.limit as i64))
        .build();

    let ignored_ids = request.ignored_ids.iter().fold(vec![], |mut acc, id| {
        let id = ID::from_string(id);
        if let Ok(id) = id {
            acc.push(id.to_bson());
        }
        acc
    });
    let mut query = doc! {
        "_id": { "$nin": ignored_ids },
    };
    if !request.project_ids.is_empty() {
        query.insert("project_ids", doc! { "$in": request.project_ids.clone() });
    }

    let filtered_types: Vec<i32> = request
        .hello_world_types
        .iter()
        .copied()
        .filter(|f| *f >= 0)
        .collect();
    if !filtered_types.is_empty() {
        query.insert("hello_world_type", doc! { "$in": filtered_types });
    }

    if request.search_term.len() >= 2 {
        let search_doc: Vec<Document> = vec![
            doc! { "name": { "$regex": &request.search_term, "$options": "i" } },
            doc! { "description": { "$regex": &request.search_term, "$options": "i" } },
        ];
        query.insert("$or", search_doc);
    }

    let cursor_result = collection.find(query, options).await;

    if let Ok(cursor) = cursor_result {
        cursor
            .then(|c| match c {
                Ok(doc) => {
                    let item_result: Option<HelloWorld> =
                        from_bson(Bson::Document(doc)).map_or_else(|_| None, Some);
                    future::ready(item_result)
                }
                Err(_) => future::ready(None),
            })
            .fold(tx, |mut tx, some_item| async move {
                if let Some(item) = some_item {
                    debug!("item: {:?}", item);
                    tx.send(Ok(item.clone())).await.unwrap();
                }
                tx
            })
            .await;
    } else {
        tx.send(Err(Status::internal("DATABASE ERROR")))
            .await
            .unwrap();
    }
    Response::new(rx)
}

pub async fn update_one(
    collection: &Collection,
    request: &UpdateHelloWorldRequest,
) -> Result<tonic::Response<HelloWorld>, tonic::Status> {
    let id = ID::from_string(&request.id)?;
    let query = doc! { "_id": id.to_bson() };

    // update if there's a mask and paths
    if let Some(mask) = &request.mask {
        if !mask.paths.is_empty() {
            let doc = mask.paths.iter().fold(doc! {}, |mut doc, path| {
                match path.as_str() {
                    "name" => doc.insert("name", request.name.to_owned()),
                    "description" => doc.insert("description", request.description.to_owned()),
                    "project_ids" => doc.insert("project_ids", request.project_ids.to_owned()),
                    "subtitle" => doc.insert("subtitle", request.subtitle.to_owned()),
                    "is_awesome" => doc.insert("is_awesome", request.is_awesome),
                    "hello_world_type" => {
                        doc.insert("hello_world_type", request.hello_world_type.to_owned())
                    }
                    _ => {
                        warn!("Path: {} is not supported", path);
                        None
                    }
                };
                doc
            });
            let result = collection
                .update_one(query, doc! { "$set": doc }, None)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            debug!("Update result: {:?}", result);
        }
    }

    // get the updated object
    get_by_id(collection, &request.id).await
}

pub async fn delete_by_id(
    collection: &Collection,
    id: &str,
) -> Result<tonic::Response<()>, tonic::Status> {
    let id = ID::from_string(id)?;
    let query = doc! { "_id": id.to_bson() };

    let _ = collection
        .delete_one(query, None)
        .await
        .map_err(|e| tonic::Status::internal(e.to_string()))?;

    Ok(Response::new(()))
}
