//! Integration tests for the generated Petstore client.
//!
//! Each test stands up a `tower::service_fn` mock in place of a real HTTP
//! server, drives one generated `Operation` through `gen::execute`, and
//! asserts on both the outbound request shape and the decoded response.

use http_body_util::Full;
use petstore_client::gen;
use petstore_client::gen::models::{NewPet, NewPetPropertyStatus, Pet, PetPropertyStatus};
use petstore_client::gen::operations::{
    CreatePet, CreatePetOutput, DeletePet, DeletePetOutput, GetPet, GetPetOutput, ListPets,
    ListPetsOutput,
};
use std::convert::Infallible;

type ReqBody = Full<bytes::Bytes>;
type RespBody = Full<bytes::Bytes>;

/// Build a JSON response with an arbitrary status code.
fn json_resp(status: u16, body: &[u8]) -> http::Response<RespBody> {
    http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Full::new(bytes::Bytes::copy_from_slice(body)))
        .unwrap()
}

/// Build an empty response (no body) with the given status.
fn empty_resp(status: u16) -> http::Response<RespBody> {
    http::Response::builder()
        .status(status)
        .body(Full::new(bytes::Bytes::new()))
        .unwrap()
}

// ---------------------------------------------------------------------------
// ListPets
// ---------------------------------------------------------------------------

/// GET /pets with no limit returns an empty array.
#[tokio::test]
async fn list_pets_empty_response() {
    let mut svc = tower::service_fn(|req: http::Request<ReqBody>| async move {
        assert_eq!(req.method(), http::Method::GET);
        assert_eq!(req.uri().path(), "/pets");
        assert!(req.uri().query().is_none(), "unexpected query string");
        Ok::<_, Infallible>(json_resp(200, b"[]"))
    });

    let out = gen::execute(&mut svc, ListPets { limit: None })
        .await
        .unwrap();
    assert!(matches!(out, ListPetsOutput::Ok(ref pets) if pets.is_empty()));
}

/// GET /pets?limit=5 appends the query parameter to the URI.
#[tokio::test]
async fn list_pets_passes_limit_query_param() {
    let mut svc = tower::service_fn(|req: http::Request<ReqBody>| async move {
        assert_eq!(req.uri().query(), Some("limit=5"));
        Ok::<_, Infallible>(json_resp(200, b"[]"))
    });

    let out = gen::execute(&mut svc, ListPets { limit: Some(5) })
        .await
        .unwrap();
    assert!(matches!(out, ListPetsOutput::Ok(_)));
}

/// GET /pets deserialises a non-empty pet list correctly.
#[tokio::test]
async fn list_pets_deserialises_pet_list() {
    let body = br#"[{"id":1,"name":"Fido","tag":"dog","status":"available"}]"#;
    let mut svc = tower::service_fn(|_req: http::Request<ReqBody>| async move {
        Ok::<_, Infallible>(json_resp(200, body))
    });

    let out = gen::execute(&mut svc, ListPets { limit: None })
        .await
        .unwrap();
    let ListPetsOutput::Ok(pets) = out;
    assert_eq!(pets.len(), 1);
    assert_eq!(pets[0].id, 1);
    assert_eq!(pets[0].name, "Fido");
    assert_eq!(pets[0].tag.as_deref(), Some("dog"));
    assert_eq!(pets[0].status, Some(PetPropertyStatus::Available));
}

/// An undeclared status code (e.g. 500) returns `Err(UndeclaredStatus)`.
#[tokio::test]
async fn list_pets_undeclared_status_returns_error() {
    let mut svc = tower::service_fn(|_req: http::Request<ReqBody>| async move {
        Ok::<_, Infallible>(json_resp(500, b"\"internal error\""))
    });

    let err = gen::execute(&mut svc, ListPets { limit: None })
        .await
        .unwrap_err();
    // ExecuteError::Operation wraps the inner ListPetsError
    let gen::ExecuteError::Operation(inner) = err;
    assert!(
        matches!(inner, petstore_client::gen::operations::ListPetsError::UndeclaredStatus { status: 500, .. }),
        "expected UndeclaredStatus(500), got {inner:?}"
    );
}

// ---------------------------------------------------------------------------
// CreatePet
// ---------------------------------------------------------------------------

/// POST /pets serialises the body as JSON and returns the created Pet.
#[tokio::test]
async fn create_pet_serialises_body_and_parses_response() {
    let mut svc = tower::service_fn(|req: http::Request<ReqBody>| async move {
        assert_eq!(req.method(), http::Method::POST);
        assert_eq!(req.uri().path(), "/pets");
        assert_eq!(
            req.headers().get(http::header::CONTENT_TYPE).unwrap(),
            "application/json"
        );

        // Verify the outbound body round-trips correctly.
        use http_body_util::BodyExt as _;
        let bytes = req.into_body().collect().await.unwrap().to_bytes();
        let sent: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(sent["name"], "Buddy");
        assert_eq!(sent["status"], "available");

        let resp_body = br#"{"id":42,"name":"Buddy","status":"available"}"#;
        Ok::<_, Infallible>(json_resp(201, resp_body))
    });

    let op = CreatePet {
        body: NewPet {
            name: "Buddy".into(),
            status: Some(NewPetPropertyStatus::Available),
            tag: None,
        },
    };
    let out = gen::execute(&mut svc, op).await.unwrap();
    let CreatePetOutput::Created(pet) = out;
    assert_eq!(pet.id, 42);
    assert_eq!(pet.name, "Buddy");
}

// ---------------------------------------------------------------------------
// GetPet
// ---------------------------------------------------------------------------

/// GET /pets/{petId} returns the pet on 200.
#[tokio::test]
async fn get_pet_returns_ok() {
    let body = br#"{"id":7,"name":"Whiskers","status":"pending"}"#;
    let mut svc = tower::service_fn(|req: http::Request<ReqBody>| async move {
        assert_eq!(req.method(), http::Method::GET);
        assert_eq!(req.uri().path(), "/pets/7");
        Ok::<_, Infallible>(json_resp(200, body))
    });

    let out = gen::execute(&mut svc, GetPet { pet_id: 7 })
        .await
        .unwrap();
    let GetPetOutput::Ok(pet) = out else {
        panic!("expected Ok, got {out:?}");
    };
    assert_eq!(pet.id, 7);
    assert_eq!(pet.name, "Whiskers");
    assert_eq!(pet.status, Some(PetPropertyStatus::Pending));
}

/// GET /pets/{petId} returns NotFound on 404.
#[tokio::test]
async fn get_pet_returns_not_found() {
    let mut svc = tower::service_fn(|_req: http::Request<ReqBody>| async move {
        Ok::<_, Infallible>(empty_resp(404))
    });

    let out = gen::execute(&mut svc, GetPet { pet_id: 99 })
        .await
        .unwrap();
    assert!(matches!(out, GetPetOutput::NotFound));
}

// ---------------------------------------------------------------------------
// DeletePet
// ---------------------------------------------------------------------------

/// DELETE /pets/{petId} returns NoContent on 204.
#[tokio::test]
async fn delete_pet_returns_no_content() {
    let mut svc = tower::service_fn(|req: http::Request<ReqBody>| async move {
        assert_eq!(req.method(), http::Method::DELETE);
        assert_eq!(req.uri().path(), "/pets/3");
        Ok::<_, Infallible>(empty_resp(204))
    });

    let out = gen::execute(&mut svc, DeletePet { pet_id: 3 })
        .await
        .unwrap();
    assert!(matches!(out, DeletePetOutput::NoContent));
}

/// DELETE /pets/{petId} returns NotFound on 404.
#[tokio::test]
async fn delete_pet_returns_not_found() {
    let mut svc = tower::service_fn(|_req: http::Request<ReqBody>| async move {
        Ok::<_, Infallible>(empty_resp(404))
    });

    let out = gen::execute(&mut svc, DeletePet { pet_id: 99 })
        .await
        .unwrap();
    assert!(matches!(out, DeletePetOutput::NotFound));
}

// ---------------------------------------------------------------------------
// Model serde
// ---------------------------------------------------------------------------

/// All three `PetPropertyStatus` variants round-trip through JSON with the
/// exact wire values declared in the spec.
#[test]
fn pet_status_enum_roundtrip() {
    for (variant, wire) in [
        (PetPropertyStatus::Available, "\"available\""),
        (PetPropertyStatus::Pending, "\"pending\""),
        (PetPropertyStatus::Sold, "\"sold\""),
    ] {
        let encoded = serde_json::to_string(&variant).unwrap();
        assert_eq!(encoded, wire);
        let decoded: PetPropertyStatus = serde_json::from_str(wire).unwrap();
        assert_eq!(decoded, variant);
    }
}

/// A full `Pet` with all optional fields set serialises and deserialises
/// without data loss.
#[test]
fn pet_full_roundtrip() {
    let pet = Pet {
        id: 1,
        name: "Rex".into(),
        tag: Some("dog".into()),
        status: Some(PetPropertyStatus::Sold),
    };
    let json = serde_json::to_string(&pet).unwrap();
    let back: Pet = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, pet.id);
    assert_eq!(back.name, pet.name);
    assert_eq!(back.tag, pet.tag);
    assert_eq!(back.status, pet.status);
}

/// Optional fields are omitted from serialisation when `None`.
#[test]
fn pet_omits_none_fields() {
    let pet = Pet {
        id: 2,
        name: "Nemo".into(),
        tag: None,
        status: None,
    };
    let json = serde_json::to_string(&pet).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(!v.as_object().unwrap().contains_key("tag"));
    assert!(!v.as_object().unwrap().contains_key("status"));
}
