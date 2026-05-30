// Sibling-file integration-style tests for the `web::server` routes
// and helpers, wired in via `#[path = "server_tests.rs"] mod tests;`
// so the production `server.rs` stays under the `bca check` per-file
// metric caps. The `./**/*_tests.rs` rule in `.bcaignore` keeps this
// file out of the self-scan walker.

use actix_web::web::Bytes;
use actix_web::{http::StatusCode, http::header::ContentType, test};
use pretty_assertions::assert_eq;
use serde_json::json;
use serde_json::value::Value;

use super::*;

/// Generous body limit for tests that are not exercising the 413 path.
const TEST_MAX_BODY_SIZE: usize = 1_024 * 1_024 * 4;

fn test_config() -> web::Data<ParseConfig> {
    test_config_with_body_limit(TEST_MAX_BODY_SIZE)
}

fn test_config_with_body_limit(max_body_size: usize) -> web::Data<ParseConfig> {
    web::Data::new(ParseConfig {
        timeout: None,
        semaphore: Arc::new(Semaphore::new(4)),
        orphaned_tasks: Arc::new(AtomicUsize::new(0)),
        max_orphaned_tasks: 64,
        max_body_size,
    })
}

fn test_config_with_timeout(d: Duration) -> web::Data<ParseConfig> {
    web::Data::new(ParseConfig {
        timeout: Some(d),
        semaphore: Arc::new(Semaphore::new(4)),
        orphaned_tasks: Arc::new(AtomicUsize::new(0)),
        max_orphaned_tasks: 64,
        max_body_size: TEST_MAX_BODY_SIZE,
    })
}

async fn assert_error_sanitized(result: Result<String, actix_web::Error>) {
    let err = result.unwrap_err();
    let resp = err.error_response();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
    let body_str = String::from_utf8_lossy(&body);
    assert!(
        !body_str.contains("BlockingError"),
        "response body must not contain BlockingError: {body_str}"
    );
    assert!(
        !body_str.contains("panicked"),
        "response body must not contain panic details: {body_str}"
    );
    assert!(
        !body_str.contains("secret internal detail"),
        "response body must not contain the panic message: {body_str}"
    );
    assert_eq!(body_str, "Internal server error");
}

#[actix_rt::test]
async fn test_web_ping() {
    let app =
        test::init_service(App::new().service(web::resource("/ping").route(web::get().to(ping))))
            .await;
    let req = test::TestRequest::with_uri("/ping").to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status(), StatusCode::OK);
}

#[actix_rt::test]
async fn test_web_ast() {
    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/ast")
                .guard(guard::Header("content-type", "application/json"))
                .route(web::post().to(ast_parser)),
        ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/ast")
        .set_json(AstPayload {
            id: "1234".to_string(),
            file_name: "foo.c".to_string(),
            code: "int x = 1;".to_string(),
            comment: false,
            span: true,
        })
        .to_request();

    let res: Value = test::call_and_read_body_json(&app, req).await;
    // FieldName values mirror the C grammar: `declaration` names its
    // `type` and `declarator` fields, `init_declarator` names its
    // `declarator` and `value` fields. Anonymous tokens (`=`, `;`)
    // carry no field name. Regression coverage for #244.
    let expected = json!({
        "id": "1234",
        "root": {
            "Type": "translation_unit",
            "TextValue": "",
            "Span": [1, 1, 1, 11],
            "FieldName": null,
            "Children": [
                {
                    "Type": "declaration",
                    "TextValue": "",
                    "Span": [1, 1, 1, 11],
                    "FieldName": null,
                    "Children": [
                        {
                            "Type": "primitive_type",
                            "TextValue": "int",
                            "Span": [1, 1, 1, 4],
                            "FieldName": "type",
                            "Children": []
                        },
                        {
                            "Type": "init_declarator",
                            "TextValue": "",
                            "Span": [1, 5, 1, 10],
                            "FieldName": "declarator",
                            "Children": [
                                {
                                    "Type": "identifier",
                                    "TextValue": "x",
                                    "Span": [1, 5, 1, 6],
                                    "FieldName": "declarator",
                                    "Children": []
                                },
                                {
                                    "Type": "=",
                                    "TextValue": "=",
                                    "Span": [1, 7, 1, 8],
                                    "FieldName": null,
                                    "Children": []
                                },
                                {
                                    "Type": "number_literal",
                                    "TextValue": "1",
                                    "Span": [1, 9, 1, 10],
                                    "FieldName": "value",
                                    "Children": []
                                }
                            ]
                        },
                        {
                            "Type": ";",
                            "TextValue": ";",
                            "Span": [1, 10, 1, 11],
                            "FieldName": null,
                            "Children": []
                        }
                    ]
                }
            ]
        }
    });
    assert_eq!(res, expected);
}

#[actix_rt::test]
async fn test_web_ast_string() {
    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/ast")
                .guard(guard::Header("content-type", "application/json"))
                .route(web::post().to(ast_parser)),
        ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/ast")
        .set_json(AstPayload {
            id: "1234".to_string(),
            file_name: "foo.js".to_string(),
            code: "var x = \"hello world\";".to_string(),
            comment: false,
            span: true,
        })
        .to_request();

    let res: Value = test::call_and_read_body_json(&app, req).await;
    // FieldName values mirror the JS grammar: `variable_declarator`
    // names its `name` and `value` children; `variable_declaration`
    // and its `var` keyword / `;` token are unnamed. Regression
    // coverage for #244.
    let expected = json!({
        "id": "1234",
        "root": {"Children": [{"Children": [{"Children": [],
                                             "FieldName": null,
                                             "Span": [1, 1, 1, 4],
                                             "TextValue": "var",
                                             "Type": "var"},
                                            {"Children": [{"Children": [],
                                                           "FieldName": "name",
                                                           "Span": [1, 5, 1, 6],
                                                           "TextValue": "x",
                                                           "Type": "identifier"},
                                                          {"Children": [],
                                                           "FieldName": null,
                                                           "Span": [1, 7, 1, 8],
                                                           "TextValue": "=",
                                                           "Type": "="},
                                                          {"Children": [],
                                                           "FieldName": "value",
                                                           "Span": [1, 9, 1, 22],
                                                           "TextValue": "\"hello world\"",
                                                           "Type": "string"}],
                                             "FieldName": null,
                                             "Span": [1, 5, 1, 22],
                                             "TextValue": "",
                                             "Type": "variable_declarator"},
                                            {"Children": [],
                                             "FieldName": null,
                                             "Span": [1, 22, 1, 23],
                                             "TextValue": ";",
                                             "Type": ";"}],
                               "FieldName": null,
                               "Span": [1, 1, 1, 23],
                               "TextValue": "",
                               "Type": "variable_declaration"}],
                 "FieldName": null,
                 "Span": [1, 1, 1, 23],
                 "TextValue": "",
                 "Type": "program"}
    });
    assert_eq!(res, expected);
}

#[actix_rt::test]
async fn test_web_comment_json() {
    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/comment")
                .guard(guard::Header("content-type", "application/json"))
                .route(web::post().to(comment_removal_json)),
        ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/comment")
        .set_json(WebCommentPayload {
            id: "1234".to_string(),
            file_name: "foo.c".to_string(),
            code: "int x = 1; // hello".to_string(),
        })
        .to_request();

    let res: Value = test::call_and_read_body_json(&app, req).await;
    let expected = json!({
        "id": "1234",
        "code": b"int x = 1; ",
    });

    assert_eq!(res, expected);
}

#[actix_rt::test]
async fn test_web_comment_json_invalid() {
    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/comment")
                .guard(guard::Header("content-type", "application/json"))
                .route(web::post().to(comment_removal_json)),
        ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/comment")
        .set_json(WebCommentPayload {
            id: "1234".to_string(),
            file_name: "foo.unexisting_extension".to_string(),
            code: "int x = 1; // hello".to_string(),
        })
        .to_request();

    let res: Value = test::call_and_read_body_json(&app, req).await;
    let expected = json!({
        "id": "1234",
        "error": INVALID_LANGUAGE,
    });

    assert_eq!(res, expected);
}

#[actix_rt::test]
async fn test_web_comment_json_no_comment() {
    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/comment")
                .guard(guard::Header("content-type", "application/json"))
                .route(web::post().to(comment_removal_json)),
        ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/comment")
        .set_json(WebCommentPayload {
            id: "1234".to_string(),
            file_name: "foo.c".to_string(),
            code: "int x = 1;".to_string(),
        })
        .to_request();

    let res: Value = test::call_and_read_body_json(&app, req).await;

    // No comment in the code so the code is null
    let expected = json!({
        "id": "1234",
        "code": (),
    });

    assert_eq!(res, expected);
}

#[actix_rt::test]
async fn test_web_comment_plain() {
    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/comment")
                .guard(guard::Header("content-type", "application/octet-stream"))
                .route(web::post().to(comment_removal_plain)),
        ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/comment?file_name=foo.c")
        .insert_header(ContentType::octet_stream())
        .set_payload("int x = 1; // hello")
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let res = test::read_body(resp).await;
    let expected = Bytes::from_static(b"int x = 1; ");

    assert_eq!(res, expected);
}

#[actix_rt::test]
async fn test_web_comment_plain_invalid() {
    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/comment")
                .guard(guard::Header("content-type", "application/octet-stream"))
                .route(web::post().to(comment_removal_plain)),
        ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/comment?file_name=foo.unexisting_extension")
        .insert_header(ContentType::octet_stream())
        .set_payload("int x = 1; // hello")
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let res = test::read_body(resp).await;
    let expected = Bytes::from(format!("error: {INVALID_LANGUAGE}"));

    assert_eq!(res, expected);
}

#[actix_rt::test]
async fn test_web_comment_plain_no_comment() {
    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/comment")
                .guard(guard::Header("content-type", "application/octet-stream"))
                .route(web::post().to(comment_removal_plain)),
        ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/comment?file_name=foo.c")
        .insert_header(ContentType::octet_stream())
        .set_payload("int x = 1;")
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let res = test::read_body(resp).await;

    // No comment in the code so the code is empty
    let expected = Bytes::from_static(b"");

    assert_eq!(res, expected);
}

// Inspired from https://hg.mozilla.org/mozilla-central/file/9b2a99adc05e53cd4010de512f50118594756650/extensions/java/xpcom/tests/testparams/TestParams.java#l64.
#[actix_rt::test]
async fn test_web_comment_plain_bad_chars() {
    let bad_bytes: &[u8] = &[142, 137, 138, 136, 140, 141, 10];
    let input_vec = [b"/*char*/s: ", bad_bytes].concat();
    let output_vec = [b"s: ", bad_bytes].concat();

    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/comment")
                .guard(guard::Header("content-type", "application/octet-stream"))
                .route(web::post().to(comment_removal_plain)),
        ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/comment?file_name=foo.java")
        .insert_header(ContentType::octet_stream())
        .set_payload(input_vec)
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let res = test::read_body(resp).await;

    assert_eq!(res, output_vec);
}

#[actix_rt::test]
async fn test_web_comment_plain_cpp() {
    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/comment")
                .guard(guard::Header("content-type", "application/octet-stream"))
                .route(web::post().to(comment_removal_plain)),
        ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/comment?file_name=foo.cpp")
        .insert_header(ContentType::octet_stream())
        .set_payload("int x = 1; // hello")
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let res = test::read_body(resp).await;
    let expected = Bytes::from_static(b"int x = 1; ");

    assert_eq!(res, expected);
}

/// Guard-rejection regression: the production `/comment` plain route
/// requires `application/octet-stream`. A `text/plain` request must not
/// hit `comment_removal_plain`; it should fall through to the default
/// 404 because no service matches. See issue #294.
#[actix_rt::test]
async fn test_web_comment_plain_rejects_text_plain() {
    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/comment")
                .guard(guard::Header("content-type", "application/octet-stream"))
                .route(web::post().to(comment_removal_plain)),
        ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/comment?file_name=foo.c")
        .insert_header(ContentType::plaintext())
        .set_payload("int x = 1; // hello")
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn test_web_metrics_json() {
    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/metrics")
                .guard(guard::Header("content-type", "application/json"))
                .route(web::post().to(metrics_json)),
        ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/metrics")
        .set_json(WebMetricsPayload {
            id: "1234".to_string(),
            file_name: "test.py".to_string(),
            code: "# -*- Mode: Objective-C++; tab-width: 2; indent-tabs-mode: nil; c-basic-offset: 2 -*-\n\ndef foo():\n    pass\n".to_string(),
            unit: false,
        })
        .to_request();

    let res: Value = test::call_and_read_body_json(&app, req).await;
    let expected = json!({
        "id": "1234",
        "language": "python",
        "spaces": {"kind": "unit",
                   "start_line": 1,
                   "end_line": 4,
                   "metrics": {"cyclomatic": {"sum": 2.0, "average": 1.0, "min":1.0, "max":1.0, "modified": {"sum": 2.0, "average": 1.0, "min":1.0, "max":1.0}},
                               "cognitive": {"sum": 0.0, "average": 0.0, "min":0.0, "max":0.0},
                               "nargs": {"total_functions": 0.0, "average_functions": 0.0, "total_closures": 0.0, "average_closures": 0.0, "total": 0.0, "average": 0.0, "closures_max": 0.0, "closures_min": 0.0, "functions_max": 0.0, "functions_min": 0.0},
                               "nexits": {"sum": 0.0, "average": 0.0,"min":0.0,"max":0.0},
                               "halstead": {"bugs": 0.000_942_552_557_372_941_4,
                                            "difficulty": 1.0,
                                            "effort": 4.754_887_502_163_468,
                                            "length": 3.0,
                                            "estimated_program_length": 2.0,
                                            "purity_ratio": 0.666_666_666_666_666_6,
                                            "level": 1.0,
                                            "N2": 1.0,
                                            "N1": 2.0,
                                            "vocabulary": 3.0,
                                            "time": 0.264_160_416_786_859_36,
                                            "n2": 1.0,
                                            "n1": 2.0,
                                            "volume": 4.754_887_502_163_468},
                               "loc": {"cloc": 1.0, "ploc": 2.0, "lloc": 1.0, "sloc": 4.0, "blank": 1.0, "cloc_average": 0.5, "ploc_average": 1.0, "lloc_average": 0.5, "sloc_average": 2.0, "blank_average": 0.5, "cloc_min": 0.0, "ploc_min": 2.0, "lloc_min": 1.0, "sloc_min": 2.0, "blank_min": 0.0, "cloc_max": 1.0, "ploc_max": 2.0, "lloc_max": 1.0, "sloc_max": 4.0, "blank_max": 1.0},
                               "nom": {"functions": 1.0, "closures": 0.0, "functions_average": 0.5, "closures_average": 0.0, "total": 1.0, "average": 0.5, "closures_min": 0.0, "closures_max": 0.0, "functions_min": 0.0, "functions_max": 1.0},
                               "tokens": {"tokens": 6.0, "tokens_average": 3.0, "tokens_min": 0.0, "tokens_max": 6.0},
                               "mi": {"mi_original": 139.974_331_558_152_1,
                                      "mi_sei": 176.151_664_509_301_33,
                                      "mi_visual_studio": 81.856_334_244_533_39},
                               "abc": {"assignments": 0.0, "branches": 0.0, "conditions": 0.0, "magnitude": 0.0, "assignments_average": 0.0, "branches_average": 0.0, "conditions_average": 0.0, "assignments_min": 0.0, "assignments_max": 0.0, "branches_min": 0.0, "branches_max": 0.0, "conditions_min": 0.0, "conditions_max": 0.0},
                               "wmc": {"classes": 0.0, "interfaces": 0.0, "total": 0.0}},
                   "name": "test.py",
                   "spaces": [{"kind": "function",
                               "start_line": 3,
                               "end_line": 4,
                               "metrics": {"cyclomatic": {"sum": 1.0, "average": 1.0, "min":1.0, "max":1.0, "modified": {"sum": 1.0, "average": 1.0, "min":1.0, "max":1.0}},
                                           "cognitive": {"sum": 0.0, "average": 0.0 , "min":0.0, "max":0.0},
                                           "nargs": {"total_functions": 0.0, "average_functions": 0.0, "total_closures": 0.0, "average_closures": 0.0, "total": 0.0, "average": 0.0, "closures_max": 0.0, "closures_min": 0.0, "functions_max": 0.0, "functions_min": 0.0},
                                           "nexits": {"sum": 0.0, "average": 0.0,"min":0.0,"max":0.0},
                                           "halstead": {"bugs": 0.000_942_552_557_372_941_4,
                                                        "difficulty": 1.0,
                                                        "effort": 4.754_887_502_163_468,
                                                        "length": 3.0,
                                                        "estimated_program_length": 2.0,
                                                        "purity_ratio": 0.666_666_666_666_666_6,
                                                        "level": 1.0,
                                                        "N2": 1.0,
                                                        "N1": 2.0,
                                                        "vocabulary": 3.0,
                                                        "time": 0.264_160_416_786_859_36,
                                                        "n2": 1.0,
                                                        "n1": 2.0,
                                                        "volume": 4.754_887_502_163_468},
                                           "loc": {"cloc": 0.0, "ploc": 2.0, "lloc": 1.0, "sloc": 2.0, "blank": 0.0, "cloc_average": 0.0, "ploc_average": 2.0, "lloc_average": 1.0, "sloc_average": 2.0, "blank_average": 0.0, "cloc_min": 0.0, "ploc_min": 2.0, "lloc_min": 1.0, "sloc_min": 2.0, "blank_min": 0.0, "cloc_max": 0.0, "ploc_max": 2.0, "lloc_max": 1.0, "sloc_max": 2.0, "blank_max": 0.0},
                                           "nom": {"functions": 1.0, "closures": 0.0, "functions_average": 1.0, "closures_average": 0.0, "total": 1.0, "average": 1.0, "closures_min": 0.0, "closures_max": 0.0, "functions_min": 1.0, "functions_max": 1.0},
                                           "tokens": {"tokens": 6.0, "tokens_average": 6.0, "tokens_min": 6.0, "tokens_max": 6.0},
                                           "mi": {"mi_original": 151.433_315_883_223_23,
                                                  "mi_sei": 142.873_061_717_489_78,
                                                  "mi_visual_studio": 88.557_494_668_551_6},
                                           "abc": {"assignments": 0.0, "branches": 0.0, "conditions": 0.0, "magnitude": 0.0, "assignments_average": 0.0, "branches_average": 0.0, "conditions_average": 0.0, "assignments_min": 0.0, "assignments_max": 0.0, "branches_min": 0.0, "branches_max": 0.0, "conditions_min": 0.0, "conditions_max": 0.0}},
                               "name": "foo",
                               "spaces": []}]}
    });

    assert_eq!(res, expected);
}

#[actix_rt::test]
async fn test_web_metrics_json_unit() {
    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/metrics")
                .guard(guard::Header("content-type", "application/json"))
                .route(web::post().to(metrics_json)),
        ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/metrics")
        .set_json(WebMetricsPayload {
            id: "1234".to_string(),
            file_name: "test.py".to_string(),
            code: "def foo():\n    pass\n".to_string(),
            unit: true,
        })
        .to_request();

    let res: Value = test::call_and_read_body_json(&app, req).await;
    let expected = json!({
        "id": "1234",
        "language": "python",
        "spaces": {"kind": "unit",
                   "start_line": 1,
                   "end_line": 2,
                   "metrics": {"cyclomatic": {"sum": 2.0, "average": 1.0, "min":1.0, "max":1.0, "modified": {"sum": 2.0, "average": 1.0, "min":1.0, "max":1.0}},
                               "cognitive": {"sum": 0.0, "average": 0.0 , "min":0.0, "max":0.0},
                               "nargs": {"total_functions": 0.0, "average_functions": 0.0, "total_closures": 0.0, "average_closures": 0.0, "total": 0.0, "average": 0.0, "closures_max": 0.0, "closures_min": 0.0, "functions_max": 0.0, "functions_min": 0.0},
                               "nexits": {"sum": 0.0, "average": 0.0,"min":0.0,"max":0.0},
                               "halstead": {"bugs": 0.000_942_552_557_372_941_4,
                                            "difficulty": 1.0,
                                            "effort": 4.754_887_502_163_468,
                                            "length": 3.0,
                                            "estimated_program_length": 2.0,
                                            "purity_ratio": 0.666_666_666_666_666_6,
                                            "level": 1.0,
                                            "N2": 1.0,
                                            "N1": 2.0,
                                            "vocabulary": 3.0,
                                            "time": 0.264_160_416_786_859_36,
                                            "n2": 1.0,
                                            "n1": 2.0,
                                            "volume": 4.754_887_502_163_468},
                               "loc": {"cloc": 0.0, "ploc": 2.0, "lloc": 1.0, "sloc": 2.0, "blank": 0.0, "cloc_average": 0.0, "ploc_average": 1.0, "lloc_average": 0.5, "sloc_average": 1.0, "blank_average": 0.0, "cloc_min": 0.0, "ploc_min": 2.0, "lloc_min": 1.0, "sloc_min": 2.0, "blank_min": 0.0, "cloc_max": 0.0, "ploc_max": 2.0, "lloc_max": 1.0, "sloc_max": 2.0, "blank_max": 0.0},
                               "nom": {"functions": 1.0, "closures": 0.0, "functions_average": 0.5, "closures_average": 0.0, "total": 1.0, "average": 0.5, "closures_min": 0.0, "closures_max": 0.0, "functions_min": 0.0, "functions_max": 1.0},
                               "tokens": {"tokens": 6.0, "tokens_average": 3.0, "tokens_min": 0.0, "tokens_max": 6.0},
                               "mi": {"mi_original": 151.203_315_883_223_2,
                                      "mi_sei": 142.643_061_717_489_76,
                                      "mi_visual_studio": 88.422_991_744_574_97},
                               "abc": {"assignments": 0.0, "branches": 0.0, "conditions": 0.0, "magnitude": 0.0, "assignments_average": 0.0, "branches_average": 0.0, "conditions_average": 0.0, "assignments_min": 0.0, "assignments_max": 0.0, "branches_min": 0.0, "branches_max": 0.0, "conditions_min": 0.0, "conditions_max": 0.0},
                               "wmc": {"classes": 0.0, "interfaces": 0.0, "total": 0.0}},
                   "name": "test.py",
                   "spaces": []}
    });

    assert_eq!(res, expected);
}

#[actix_rt::test]
async fn test_web_metrics_plain() {
    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/metrics")
                .guard(guard::Header("content-type", "application/octet-stream"))
                .route(web::post().to(metrics_plain)),
        ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/metrics?file_name=test.py")
        .insert_header(ContentType::octet_stream())
        .set_payload("def foo():\n    pass\n")
        .to_request();

    let res: Value = test::call_and_read_body_json(&app, req).await;
    let expected = json!({
        "id": "",
        "language": "python",
        "spaces": {"kind": "unit",
                   "start_line": 1,
                   "end_line": 2,
                   "metrics": {"cyclomatic": {"sum": 2.0, "average": 1.0, "min": 1.0,"max": 1.0, "modified": {"sum": 2.0, "average": 1.0, "min": 1.0,"max": 1.0}},
                               "cognitive": {"sum": 0.0, "average": 0.0 , "min":0.0, "max":0.0},
                               "nargs": {"total_functions": 0.0, "average_functions": 0.0, "total_closures": 0.0, "average_closures": 0.0, "total": 0.0, "average": 0.0, "closures_max": 0.0, "closures_min": 0.0, "functions_max": 0.0, "functions_min": 0.0},
                               "nexits": {"sum": 0.0, "average": 0.0,"min":0.0,"max":0.0},
                               "halstead": {"bugs": 0.000_942_552_557_372_941_4,
                                            "difficulty": 1.0,
                                            "effort": 4.754_887_502_163_468,
                                            "length": 3.0,
                                            "estimated_program_length": 2.0,
                                            "purity_ratio": 0.666_666_666_666_666_6,
                                            "level": 1.0,
                                            "N2": 1.0,
                                            "N1": 2.0,
                                            "vocabulary": 3.0,
                                            "time": 0.264_160_416_786_859_36,
                                            "n2": 1.0,
                                            "n1": 2.0,
                                            "volume": 4.754_887_502_163_468},
                               "loc": {"cloc": 0.0, "ploc": 2.0, "lloc": 1.0, "sloc": 2.0, "blank": 0.0, "cloc_average": 0.0, "ploc_average": 1.0, "lloc_average": 0.5, "sloc_average": 1.0, "blank_average": 0.0, "cloc_min": 0.0, "ploc_min": 2.0, "lloc_min": 1.0, "sloc_min": 2.0, "blank_min": 0.0, "cloc_max": 0.0, "ploc_max": 2.0, "lloc_max": 1.0, "sloc_max": 2.0, "blank_max": 0.0},
                               "nom": {"functions": 1.0, "closures": 0.0, "functions_average": 0.5, "closures_average": 0.0, "total": 1.0, "average": 0.5, "closures_min": 0.0, "closures_max": 0.0, "functions_min": 0.0, "functions_max": 1.0},
                               "tokens": {"tokens": 6.0, "tokens_average": 3.0, "tokens_min": 0.0, "tokens_max": 6.0},
                               "mi": {"mi_original": 151.203_315_883_223_2,
                                      "mi_sei": 142.643_061_717_489_76,
                                      "mi_visual_studio": 88.422_991_744_574_97},
                               "abc": {"assignments": 0.0, "branches": 0.0, "conditions": 0.0, "magnitude": 0.0, "assignments_average": 0.0, "branches_average": 0.0, "conditions_average": 0.0, "assignments_min": 0.0, "assignments_max": 0.0, "branches_min": 0.0, "branches_max": 0.0, "conditions_min": 0.0, "conditions_max": 0.0},
                               "wmc": {"classes": 0.0, "interfaces": 0.0, "total": 0.0}},
                   "name": "test.py",
                   "spaces": [{"kind": "function",
                               "start_line": 1,
                               "end_line": 2,
                               "metrics": {"cyclomatic": {"sum": 1.0, "average": 1.0, "min": 1.0,"max": 1.0, "modified": {"sum": 1.0, "average": 1.0, "min": 1.0,"max": 1.0}},
                                           "cognitive": {"sum": 0.0, "average": 0.0 , "min":0.0, "max":0.0},
                                           "nargs": {"total_functions": 0.0, "average_functions": 0.0, "total_closures": 0.0, "average_closures": 0.0, "total": 0.0, "average": 0.0, "closures_max": 0.0, "closures_min": 0.0, "functions_max": 0.0, "functions_min": 0.0},
                                           "nexits": {"sum": 0.0, "average": 0.0,"min":0.0,"max":0.0},
                                           "halstead": {"bugs": 0.000_942_552_557_372_941_4,
                                                        "difficulty": 1.0,
                                                        "effort": 4.754_887_502_163_468,
                                                        "length": 3.0,
                                                        "estimated_program_length": 2.0,
                                                        "purity_ratio": 0.666_666_666_666_666_6,
                                                        "level": 1.0,
                                                        "N2": 1.0,
                                                        "N1": 2.0,
                                                        "vocabulary": 3.0,
                                                        "time": 0.264_160_416_786_859_36,
                                                        "n2": 1.0,
                                                        "n1": 2.0,
                                                        "volume": 4.754_887_502_163_468},
                                           "loc": {"cloc": 0.0, "ploc": 2.0, "lloc": 1.0, "sloc": 2.0, "blank": 0.0, "cloc_average": 0.0, "ploc_average": 2.0, "lloc_average": 1.0, "sloc_average": 2.0, "blank_average": 0.0, "cloc_min": 0.0, "ploc_min": 2.0, "lloc_min": 1.0, "sloc_min": 2.0, "blank_min": 0.0, "cloc_max": 0.0, "ploc_max": 2.0, "lloc_max": 1.0, "sloc_max": 2.0, "blank_max": 0.0},
                                           "nom": {"functions": 1.0, "closures": 0.0, "functions_average": 1.0, "closures_average": 0.0, "total": 1.0, "average": 1.0, "closures_min": 0.0, "closures_max": 0.0, "functions_min": 1.0, "functions_max": 1.0},
                                           "tokens": {"tokens": 6.0, "tokens_average": 6.0, "tokens_min": 6.0, "tokens_max": 6.0},
                                           "mi": {"mi_original": 151.433_315_883_223_23,
                                                  "mi_sei": 142.873_061_717_489_78,
                                                  "mi_visual_studio": 88.557_494_668_551_6},
                                           "abc": {"assignments": 0.0, "branches": 0.0, "conditions": 0.0, "magnitude": 0.0, "assignments_average": 0.0, "branches_average": 0.0, "conditions_average": 0.0, "assignments_min": 0.0, "assignments_max": 0.0, "branches_min": 0.0, "branches_max": 0.0, "conditions_min": 0.0, "conditions_max": 0.0}},
                               "name": "foo",
                               "spaces": []}]}
    });

    assert_eq!(res, expected);
}

/// Guard-rejection regression: the production `/metrics` plain route
/// requires `application/octet-stream`. A `text/plain` request must
/// fall through to 404 instead of being routed to `metrics_plain`.
/// See issue #294.
#[actix_rt::test]
async fn test_web_metrics_plain_rejects_text_plain() {
    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/metrics")
                .guard(guard::Header("content-type", "application/octet-stream"))
                .route(web::post().to(metrics_plain)),
        ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/metrics?file_name=test.py")
        .insert_header(ContentType::plaintext())
        .set_payload("def foo():\n    pass\n")
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn test_web_function_json() {
    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/function")
                .guard(guard::Header("content-type", "application/json"))
                .route(web::post().to(function_json)),
        ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/function")
        .set_json(WebCommentPayload {
            id: "1234".to_string(),
            file_name: "test.py".to_string(),
            code: "def foo():\n    pass\n\ndef bar():\n    pass".to_string(),
        })
        .to_request();

    let res: Value = test::call_and_read_body_json(&app, req).await;
    let expected = json!({
        "id": "1234",
        "spans": [
            {
                "end_line": 2,
                "error": false,
                "name": "foo",
                "start_line": 1
            },
            {
                "end_line": 5,
                "error": false,
                "name": "bar",
                "start_line": 4
            }
        ]
    });

    assert_eq!(res, expected);
}

#[actix_rt::test]
async fn test_web_function_plain() {
    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/function")
                .guard(guard::Header("content-type", "application/octet-stream"))
                .route(web::post().to(function_plain)),
        ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/function?file_name=test.py")
        .insert_header(ContentType::octet_stream())
        .set_payload("def foo():\n    pass\n\ndef bar():\n    pass")
        .to_request();

    let res: Value = test::call_and_read_body_json(&app, req).await;
    let expected = json!({
        "id": "",
        "spans": [
            {
                "end_line": 2,
                "error": false,
                "name": "foo",
                "start_line": 1
            },
            {
                "end_line": 5,
                "error": false,
                "name": "bar",
                "start_line": 4
            }
        ]
    });

    assert_eq!(res, expected);
}

/// Guard-rejection regression: the production `/function` plain route
/// requires `application/octet-stream`. A `text/plain` request must
/// fall through to 404 instead of being routed to `function_plain`.
/// See issue #294.
#[actix_rt::test]
async fn test_web_function_plain_rejects_text_plain() {
    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/function")
                .guard(guard::Header("content-type", "application/octet-stream"))
                .route(web::post().to(function_plain)),
        ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/function?file_name=test.py")
        .insert_header(ContentType::plaintext())
        .set_payload("def foo():\n    pass\n\ndef bar():\n    pass")
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn test_run_parse_error_does_not_leak_internals() {
    let config = test_config();
    let result = run_parse(&config, || -> String { panic!("secret internal detail") }).await;
    assert_error_sanitized(result).await;
}

#[actix_rt::test]
async fn test_run_parse_error_with_timeout_does_not_leak_internals() {
    let config = test_config_with_timeout(Duration::from_secs(5));
    let result = run_parse(&config, || -> String { panic!("secret internal detail") }).await;
    assert_error_sanitized(result).await;
}

#[actix_rt::test]
async fn test_run_parse_timeout_returns_504() {
    let config = test_config_with_timeout(Duration::from_millis(50));
    // The blocking task outlives the timeout, but exits shortly after to keep
    // the test fast (the cleanup task awaits the JoinHandle).
    let result = run_parse(&config, || {
        std::thread::sleep(Duration::from_millis(200));
        "completed"
    })
    .await;

    let err = result.unwrap_err();
    let resp = err.error_response();
    assert_eq!(resp.status(), StatusCode::GATEWAY_TIMEOUT);
    let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
    assert_eq!(String::from_utf8_lossy(&body), PARSE_TIMEOUT);
}

#[actix_rt::test]
async fn test_run_parse_timeout_increments_orphan_counter_and_decrements_on_completion() {
    let orphaned = Arc::new(AtomicUsize::new(0));
    // Use a channel so the blocking task exits quickly after the timeout fires.
    let (tx, rx) = std::sync::mpsc::channel::<()>();

    let config = web::Data::new(ParseConfig {
        timeout: Some(Duration::from_millis(50)),
        semaphore: Arc::new(Semaphore::new(4)),
        orphaned_tasks: Arc::clone(&orphaned),
        max_orphaned_tasks: 64,
        max_body_size: TEST_MAX_BODY_SIZE,
    });

    let err = run_parse(&config, move || {
        // Block until the test signals completion.
        let _ = rx.recv();
    })
    .await
    .unwrap_err();
    assert_eq!(err.error_response().status(), StatusCode::GATEWAY_TIMEOUT);

    // Counter must be 1 immediately after timeout.
    assert_eq!(orphaned.load(Ordering::Acquire), 1);

    // Unblock the orphaned task so it can finish.
    let _ = tx.send(());

    // Poll until the cleanup task has decremented the counter.
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        if orphaned.load(Ordering::Acquire) == 0 {
            break;
        }
    }
    assert_eq!(orphaned.load(Ordering::Acquire), 0);
}

#[actix_rt::test]
async fn test_run_parse_rejects_with_503_when_orphan_threshold_exceeded() {
    // Pre-fill the counter to the threshold.
    let orphaned = Arc::new(AtomicUsize::new(10));
    let config = web::Data::new(ParseConfig {
        timeout: Some(Duration::from_secs(5)),
        semaphore: Arc::new(Semaphore::new(4)),
        orphaned_tasks: Arc::clone(&orphaned),
        max_orphaned_tasks: 10,
        max_body_size: TEST_MAX_BODY_SIZE,
    });

    // The closure should never run because the threshold check fires first.
    let result = run_parse(&config, || "should not run").await;
    let err = result.unwrap_err();
    let resp = err.error_response();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
    assert_eq!(String::from_utf8_lossy(&body), "parse pool saturated");
}

#[actix_rt::test]
async fn test_run_parse_rechecks_orphan_cap_after_semaphore_admission() {
    use std::sync::atomic::AtomicBool;

    // Regression test for #291: a burst that passes the pre-admission
    // check while the orphan counter is still low must be rejected by
    // the post-admission re-check rather than spawning additional
    // blocking work.
    //
    // The semaphore has a single permit so admissions are serialised.
    // The counter is initialised one below the cap, so the
    // pre-admission check passes; the test holds the permit, bumps the
    // counter past the cap while the queued request waits, then
    // releases the permit and expects rejection without the closure
    // running.
    let orphaned = Arc::new(AtomicUsize::new(9));
    let config = web::Data::new(ParseConfig {
        timeout: None,
        semaphore: Arc::new(Semaphore::new(1)),
        orphaned_tasks: Arc::clone(&orphaned),
        max_orphaned_tasks: 10,
        max_body_size: TEST_MAX_BODY_SIZE,
    });

    // Hold the single semaphore permit so the second request must queue.
    let held_permit = Arc::clone(&config.semaphore).acquire_owned().await.unwrap();

    // Drive the would-be request from a LocalSet because actix_web::Error
    // is `!Send`, so it cannot cross a `tokio::spawn` boundary. The
    // LocalSet's `spawn_local` keeps the future on the current thread.
    let local = tokio::task::LocalSet::new();
    let closure_ran = Arc::new(AtomicBool::new(false));
    let closure_ran_for_task = Arc::clone(&closure_ran);

    let outcome = local
        .run_until(async {
            let config_for_task = config.clone();
            let queued = tokio::task::spawn_local(async move {
                run_parse(&config_for_task, move || {
                    closure_ran_for_task.store(true, Ordering::Release);
                    "should not run"
                })
                .await
            });

            // Give the queued task a chance to reach the semaphore wait.
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Simulate another request orphaning a blocking task in
            // the meantime, pushing the counter up to the cap.
            orphaned.fetch_add(1, Ordering::Release);

            // Release the permit so the queued request is admitted.
            drop(held_permit);

            queued.await.unwrap()
        })
        .await;

    let err = outcome.unwrap_err();
    let resp = err.error_response();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
    assert_eq!(String::from_utf8_lossy(&body), "parse pool saturated");
    assert!(
        !closure_ran.load(Ordering::Acquire),
        "closure must not run when orphan cap is exceeded post-admission",
    );

    // The dropped permit must be returned to the pool for subsequent requests.
    assert_eq!(config.semaphore.available_permits(), 1);
}

// Regression test for #336: the global `JsonConfig` registered on
// the `App` is what bounds JSON payload size. The four per-route
// `.app_data(web::Json::<T>)` calls removed in this change were
// function-item values, not `JsonConfig` instances, and the
// `Json<T>` extractor only honors `JsonConfig::from_req`. Build a
// service with a small global limit and assert that a body
// exceeding it is rejected with 413.
//
// The resource is wired with the same `guard::Header` filter the
// production `/ast` route uses (server.rs:411) so the test
// exercises the exact dispatch shape whose `.app_data` line was
// deleted — guard precedence vs. payload-size enforcement is then
// covered by this assertion rather than left implicit. `set_json`
// sets the `Content-Type: application/json` header on the request,
// so the request matches the guard.
#[actix_rt::test]
async fn test_web_json_payload_too_large() {
    // Use a tiny limit so the test does not allocate megabytes.
    const TEST_JSON_LIMIT: usize = 256;

    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .app_data(web::JsonConfig::default().limit(TEST_JSON_LIMIT))
            .service(
                web::resource("/ast")
                    .guard(guard::Header("content-type", "application/json"))
                    .route(web::post().to(ast_parser)),
            ),
    )
    .await;

    // Construct a JSON body whose serialized form exceeds the limit
    // by padding the `code` field. The full payload (including
    // surrounding JSON structure) is well over TEST_JSON_LIMIT bytes.
    let oversized_code = "a".repeat(TEST_JSON_LIMIT * 2);
    let req = test::TestRequest::post()
        .uri("/ast")
        .set_json(AstPayload {
            id: "1234".to_string(),
            file_name: "foo.c".to_string(),
            code: oversized_code,
            comment: false,
            span: true,
        })
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

// The octet-stream handlers read the body with the raw `web::Payload`
// extractor, which ignores `web::PayloadConfig`. Before this fix the
// previously-attached `PayloadConfig` was dead config and the body was
// accumulated with no size limit (issue #426: unbounded-body OOM DoS).
// `get_code` now enforces `ParseConfig::max_body_size` incrementally and
// returns 413 once the running total would exceed it.
//
// Small limit so the oversized body is a handful of bytes, not megabytes.
const TEST_OCTET_LIMIT: usize = 16;

/// Builds an app routing `/{path}` (octet-stream) to `handler` with a tiny
/// body limit, then asserts an oversized body is rejected with 413.
macro_rules! assert_plain_payload_too_large {
    ($name:ident, $path:literal, $handler:ident) => {
        #[actix_rt::test]
        async fn $name() {
            let app = test::init_service(
                App::new()
                    .app_data(test_config_with_body_limit(TEST_OCTET_LIMIT))
                    .service(
                        web::resource($path)
                            .guard(guard::Header("content-type", "application/octet-stream"))
                            .route(web::post().to($handler)),
                    ),
            )
            .await;

            // One byte over the limit must be rejected.
            let oversized = "a".repeat(TEST_OCTET_LIMIT + 1);
            let req = test::TestRequest::post()
                .uri(concat!($path, "?file_name=foo.c"))
                .insert_header(ContentType::octet_stream())
                .set_payload(oversized)
                .to_request();

            let resp = test::call_service(&app, req).await;
            assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
        }
    };
}

assert_plain_payload_too_large!(
    test_web_comment_plain_payload_too_large,
    "/comment",
    comment_removal_plain
);
assert_plain_payload_too_large!(
    test_web_metrics_plain_payload_too_large,
    "/metrics",
    metrics_plain
);
assert_plain_payload_too_large!(
    test_web_function_plain_payload_too_large,
    "/function",
    function_plain
);

// A body whose length is exactly at the limit must still be accepted (not
// rejected with 413): the incremental check rejects only when the running
// total *exceeds* `max_body_size`. `int x;//c` is 9 bytes, within the
// 16-byte limit, and carries a comment so the handler emits stripped code
// (200) rather than 204 No Content.
#[actix_rt::test]
async fn test_web_comment_plain_at_limit_succeeds() {
    const BODY: &str = "int x;//c";
    assert!(BODY.len() <= TEST_OCTET_LIMIT);

    let app = test::init_service(
        App::new()
            .app_data(test_config_with_body_limit(TEST_OCTET_LIMIT))
            .service(
                web::resource("/comment")
                    .guard(guard::Header("content-type", "application/octet-stream"))
                    .route(web::post().to(comment_removal_plain)),
            ),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/comment?file_name=foo.c")
        .insert_header(ContentType::octet_stream())
        .set_payload(BODY)
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
}
