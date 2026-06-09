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
use tracing_test::traced_test;

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
    // field_name values mirror the C grammar: `declaration` names its
    // `type` and `declarator` fields, `init_declarator` names its
    // `declarator` and `value` fields. Anonymous tokens (`=`, `;`)
    // carry no field name. Regression coverage for #244. Span is a
    // flat named object `{start_row, start_col, end_row, end_col}`
    // (#535); the four values preserve the former tuple order.
    let span =
        |sr, sc, er, ec| json!({"start_row": sr, "start_col": sc, "end_row": er, "end_col": ec});
    let expected = json!({
        "id": "1234",
        "root": {
            "type": "translation_unit",
            "value": "",
            "span": span(1, 1, 1, 11),
            "field_name": null,
            "children": [
                {
                    "type": "declaration",
                    "value": "",
                    "span": span(1, 1, 1, 11),
                    "field_name": null,
                    "children": [
                        {
                            "type": "primitive_type",
                            "value": "int",
                            "span": span(1, 1, 1, 4),
                            "field_name": "type",
                            "children": []
                        },
                        {
                            "type": "init_declarator",
                            "value": "",
                            "span": span(1, 5, 1, 10),
                            "field_name": "declarator",
                            "children": [
                                {
                                    "type": "identifier",
                                    "value": "x",
                                    "span": span(1, 5, 1, 6),
                                    "field_name": "declarator",
                                    "children": []
                                },
                                {
                                    "type": "=",
                                    "value": "=",
                                    "span": span(1, 7, 1, 8),
                                    "field_name": null,
                                    "children": []
                                },
                                {
                                    "type": "number_literal",
                                    "value": "1",
                                    "span": span(1, 9, 1, 10),
                                    "field_name": "value",
                                    "children": []
                                }
                            ]
                        },
                        {
                            "type": ";",
                            "value": ";",
                            "span": span(1, 10, 1, 11),
                            "field_name": null,
                            "children": []
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
    // field_name values mirror the JS grammar: `variable_declarator`
    // names its `name` and `value` children; `variable_declaration`
    // and its `var` keyword / `;` token are unnamed. Regression
    // coverage for #244. Span is a flat named object (#535).
    let span =
        |sr, sc, er, ec| json!({"start_row": sr, "start_col": sc, "end_row": er, "end_col": ec});
    let expected = json!({
        "id": "1234",
        "root": {"children": [{"children": [{"children": [],
                                             "field_name": null,
                                             "span": span(1, 1, 1, 4),
                                             "value": "var",
                                             "type": "var"},
                                            {"children": [{"children": [],
                                                           "field_name": "name",
                                                           "span": span(1, 5, 1, 6),
                                                           "value": "x",
                                                           "type": "identifier"},
                                                          {"children": [],
                                                           "field_name": null,
                                                           "span": span(1, 7, 1, 8),
                                                           "value": "=",
                                                           "type": "="},
                                                          {"children": [],
                                                           "field_name": "value",
                                                           "span": span(1, 9, 1, 22),
                                                           "value": "\"hello world\"",
                                                           "type": "string"}],
                                             "field_name": null,
                                             "span": span(1, 5, 1, 22),
                                             "value": "",
                                             "type": "variable_declarator"},
                                            {"children": [],
                                             "field_name": null,
                                             "span": span(1, 22, 1, 23),
                                             "value": ";",
                                             "type": ";"}],
                               "field_name": null,
                               "span": span(1, 1, 1, 23),
                               "value": "",
                               "type": "variable_declaration"}],
                 "field_name": null,
                 "span": span(1, 1, 1, 23),
                 "value": "",
                 "type": "program"}
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
        "language": "cpp",
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

    // No comment in the code, so `code` is the empty byte array — the
    // success envelope stays uniform with a non-empty result (#558).
    let expected = json!({
        "id": "1234",
        "language": "cpp",
        "code": [],
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

    // Errors on the octet-stream endpoint now use the uniform JSON
    // `{error, id}` body (#541), not a bare `text/plain` string.
    let body: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
    let expected = json!({
        "error": INVALID_LANGUAGE,
        "id": "",
    });

    assert_eq!(body, expected);
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
    // No comments to strip now returns `200` with an empty body, mirroring
    // the JSON variant's `200 {code: []}` rather than `204` (#558).
    assert_eq!(resp.status(), StatusCode::OK);

    let res = test::read_body(resp).await;
    let expected = Bytes::from_static(b"");

    assert_eq!(res, expected);
}

/// Empty-result parity (#558): an input with no removable comments must
/// return the *same* status code across both content-type variants, with
/// a uniform empty payload (JSON `{code: []}`, octet-stream empty body).
#[actix_rt::test]
async fn test_web_comment_empty_result_parity() {
    let json_app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/comment")
                .guard(guard::Header("content-type", "application/json"))
                .route(web::post().to(comment_removal_json)),
        ),
    )
    .await;
    let json_req = test::TestRequest::post()
        .uri("/comment")
        .set_json(WebCommentPayload {
            id: "1234".to_string(),
            file_name: "foo.c".to_string(),
            code: "int x = 1;".to_string(),
        })
        .to_request();
    let json_resp = test::call_service(&json_app, json_req).await;
    let json_status = json_resp.status();
    let json_body: Value =
        serde_json::from_slice(&test::read_body(json_resp).await).expect("JSON body");

    let plain_app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/comment")
                .guard(guard::Header("content-type", "application/octet-stream"))
                .route(web::post().to(comment_removal_plain)),
        ),
    )
    .await;
    let plain_req = test::TestRequest::post()
        .uri("/comment?file_name=foo.c")
        .insert_header(ContentType::octet_stream())
        .set_payload("int x = 1;")
        .to_request();
    let plain_resp = test::call_service(&plain_app, plain_req).await;
    let plain_status = plain_resp.status();
    let plain_body = test::read_body(plain_resp).await;

    // Both variants agree on the status code for the empty result.
    assert_eq!(json_status, StatusCode::OK);
    assert_eq!(plain_status, StatusCode::OK);
    assert_eq!(json_status, plain_status);

    // Both variants agree on what "empty" looks like: the JSON `code`
    // key is the empty byte array; the octet-stream body is empty bytes.
    assert_eq!(
        json_body,
        json!({ "id": "1234", "language": "cpp", "code": [] })
    );
    assert_eq!(plain_body, Bytes::from_static(b""));
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
                   "metrics": {"cyclomatic": {"sum": 2, "average": 2.0, "min":1, "max":1, "modified": {"sum": 2, "average": 2.0, "min":1, "max":1}},
                               "cognitive": {"sum": 0, "average": 0.0, "min":0, "max":0},
                               "nargs": {"function_args": 0, "function_args_average": 0.0, "closure_args": 0, "closure_args_average": 0.0, "total": 0, "average": 0.0, "closure_args_max": 0, "closure_args_min": 0, "function_args_max": 0, "function_args_min": 0},
                               "nexits": {"sum": 0, "average": 0.0,"min":0,"max":0},
                               "halstead": {"bugs": 0.000_942_552_557_372_941_4,
                                            "difficulty": 1.0,
                                            "effort": 4.754_887_502_163_468,
                                            "length": 3,
                                            "estimated_program_length": 2.0,
                                            "purity_ratio": 0.666_666_666_666_666_6,
                                            "level": 1.0,
                                            "total_operands": 1,
                                            "total_operators": 2,
                                            "vocabulary": 3,
                                            "time": 0.264_160_416_786_859_36,
                                            "unique_operands": 1,
                                            "unique_operators": 2,
                                            "volume": 4.754_887_502_163_468},
                               "loc": {"cloc": 1, "ploc": 2, "lloc": 1, "sloc": 4, "blank": 1, "cloc_average": 0.5, "ploc_average": 1.0, "lloc_average": 0.5, "sloc_average": 2.0, "blank_average": 0.5, "cloc_min": 0, "ploc_min": 2, "lloc_min": 1, "sloc_min": 2, "blank_min": 0, "cloc_max": 1, "ploc_max": 2, "lloc_max": 1, "sloc_max": 4, "blank_max": 1},
                               "nom": {"functions": 1, "closures": 0, "functions_average": 0.5, "closures_average": 0.0, "total": 1, "average": 0.5, "closures_min": 0, "closures_max": 0, "functions_min": 0, "functions_max": 1},
                               "tokens": {"tokens": 6, "tokens_average": 3.0, "tokens_min": 0, "tokens_max": 6},
                               "mi": {"original": 139.974_331_558_152_1,
                                      "sei": 176.151_664_509_301_33,
                                      "visual_studio": 81.856_334_244_533_39},
                               "abc": {"assignments": 0, "branches": 0, "conditions": 0, "magnitude": 0.0, "assignments_average": 0.0, "branches_average": 0.0, "conditions_average": 0.0, "assignments_min": 0, "assignments_max": 0, "branches_min": 0, "branches_max": 0, "conditions_min": 0, "conditions_max": 0},
                               "wmc": {"classes": 0, "interfaces": 0, "total": 0}},
                   "name": "test.py",
                   "spaces": [{"kind": "function",
                               "start_line": 3,
                               "end_line": 4,
                               "metrics": {"cyclomatic": {"sum": 1, "average": 1.0, "min":1, "max":1, "modified": {"sum": 1, "average": 1.0, "min":1, "max":1}},
                                           "cognitive": {"sum": 0, "average": 0.0 , "min":0, "max":0},
                                           "nargs": {"function_args": 0, "function_args_average": 0.0, "closure_args": 0, "closure_args_average": 0.0, "total": 0, "average": 0.0, "closure_args_max": 0, "closure_args_min": 0, "function_args_max": 0, "function_args_min": 0},
                                           "nexits": {"sum": 0, "average": 0.0,"min":0,"max":0},
                                           "halstead": {"bugs": 0.000_942_552_557_372_941_4,
                                                        "difficulty": 1.0,
                                                        "effort": 4.754_887_502_163_468,
                                                        "length": 3,
                                                        "estimated_program_length": 2.0,
                                                        "purity_ratio": 0.666_666_666_666_666_6,
                                                        "level": 1.0,
                                                        "total_operands": 1,
                                                        "total_operators": 2,
                                                        "vocabulary": 3,
                                                        "time": 0.264_160_416_786_859_36,
                                                        "unique_operands": 1,
                                                        "unique_operators": 2,
                                                        "volume": 4.754_887_502_163_468},
                                           "loc": {"cloc": 0, "ploc": 2, "lloc": 1, "sloc": 2, "blank": 0, "cloc_average": 0.0, "ploc_average": 2.0, "lloc_average": 1.0, "sloc_average": 2.0, "blank_average": 0.0, "cloc_min": 0, "ploc_min": 2, "lloc_min": 1, "sloc_min": 2, "blank_min": 0, "cloc_max": 0, "ploc_max": 2, "lloc_max": 1, "sloc_max": 2, "blank_max": 0},
                                           "nom": {"functions": 1, "closures": 0, "functions_average": 1.0, "closures_average": 0.0, "total": 1, "average": 1.0, "closures_min": 0, "closures_max": 0, "functions_min": 1, "functions_max": 1},
                                           "tokens": {"tokens": 6, "tokens_average": 6.0, "tokens_min": 6, "tokens_max": 6},
                                           "mi": {"original": 151.433_315_883_223_23,
                                                  "sei": 142.873_061_717_489_78,
                                                  "visual_studio": 88.557_494_668_551_6},
                                           "abc": {"assignments": 0, "branches": 0, "conditions": 0, "magnitude": 0.0, "assignments_average": 0.0, "branches_average": 0.0, "conditions_average": 0.0, "assignments_min": 0, "assignments_max": 0, "branches_min": 0, "branches_max": 0, "conditions_min": 0, "conditions_max": 0}},
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
                   "metrics": {"cyclomatic": {"sum": 2, "average": 2.0, "min":1, "max":1, "modified": {"sum": 2, "average": 2.0, "min":1, "max":1}},
                               "cognitive": {"sum": 0, "average": 0.0 , "min":0, "max":0},
                               "nargs": {"function_args": 0, "function_args_average": 0.0, "closure_args": 0, "closure_args_average": 0.0, "total": 0, "average": 0.0, "closure_args_max": 0, "closure_args_min": 0, "function_args_max": 0, "function_args_min": 0},
                               "nexits": {"sum": 0, "average": 0.0,"min":0,"max":0},
                               "halstead": {"bugs": 0.000_942_552_557_372_941_4,
                                            "difficulty": 1.0,
                                            "effort": 4.754_887_502_163_468,
                                            "length": 3,
                                            "estimated_program_length": 2.0,
                                            "purity_ratio": 0.666_666_666_666_666_6,
                                            "level": 1.0,
                                            "total_operands": 1,
                                            "total_operators": 2,
                                            "vocabulary": 3,
                                            "time": 0.264_160_416_786_859_36,
                                            "unique_operands": 1,
                                            "unique_operators": 2,
                                            "volume": 4.754_887_502_163_468},
                               "loc": {"cloc": 0, "ploc": 2, "lloc": 1, "sloc": 2, "blank": 0, "cloc_average": 0.0, "ploc_average": 1.0, "lloc_average": 0.5, "sloc_average": 1.0, "blank_average": 0.0, "cloc_min": 0, "ploc_min": 2, "lloc_min": 1, "sloc_min": 2, "blank_min": 0, "cloc_max": 0, "ploc_max": 2, "lloc_max": 1, "sloc_max": 2, "blank_max": 0},
                               "nom": {"functions": 1, "closures": 0, "functions_average": 0.5, "closures_average": 0.0, "total": 1, "average": 0.5, "closures_min": 0, "closures_max": 0, "functions_min": 0, "functions_max": 1},
                               "tokens": {"tokens": 6, "tokens_average": 3.0, "tokens_min": 0, "tokens_max": 6},
                               "mi": {"original": 151.203_315_883_223_2,
                                      "sei": 142.643_061_717_489_76,
                                      "visual_studio": 88.422_991_744_574_97},
                               "abc": {"assignments": 0, "branches": 0, "conditions": 0, "magnitude": 0.0, "assignments_average": 0.0, "branches_average": 0.0, "conditions_average": 0.0, "assignments_min": 0, "assignments_max": 0, "branches_min": 0, "branches_max": 0, "conditions_min": 0, "conditions_max": 0},
                               "wmc": {"classes": 0, "interfaces": 0, "total": 0}},
                   "name": "test.py",
                   "spaces": []}
    });

    assert_eq!(res, expected);
}

// Regression for #540: the `/metrics` `language` field must report the
// canonical lowercase slug — a valid `LANG::from_str` lookup token —
// not a human-pretty display form. Before the fix a `.cpp` file
// surfaced `"c/c++"` and a `.cs` file `"c#"`, neither of which a client
// could feed back through the language vocabulary. `parse::<LANG>()`
// here proves the reported value round-trips.
#[actix_rt::test]
async fn test_web_metrics_language_is_canonical_slug() {
    use big_code_analysis::LANG;

    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/metrics")
                .guard(guard::Header("content-type", "application/json"))
                .route(web::post().to(metrics_json)),
        ),
    )
    .await;

    for (file_name, code, expected_slug, expected_lang) in [
        ("a.cpp", "int x = 1;", "cpp", LANG::Cpp),
        ("a.cs", "class C { int x; }", "csharp", LANG::Csharp),
    ] {
        let req = test::TestRequest::post()
            .uri("/metrics")
            .set_json(WebMetricsPayload {
                id: "lang".to_string(),
                file_name: file_name.to_string(),
                code: code.to_string(),
                unit: false,
            })
            .to_request();
        let res: Value = test::call_and_read_body_json(&app, req).await;
        assert_eq!(
            res["language"],
            json!(expected_slug),
            "{file_name} should report the canonical slug",
        );
        assert_eq!(
            expected_slug.parse::<LANG>(),
            Ok(expected_lang),
            "reported language `{expected_slug}` must round-trip via FromStr",
        );
    }
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
                   "metrics": {"cyclomatic": {"sum": 2, "average": 2.0, "min": 1,"max": 1, "modified": {"sum": 2, "average": 2.0, "min": 1,"max": 1}},
                               "cognitive": {"sum": 0, "average": 0.0 , "min":0, "max":0},
                               "nargs": {"function_args": 0, "function_args_average": 0.0, "closure_args": 0, "closure_args_average": 0.0, "total": 0, "average": 0.0, "closure_args_max": 0, "closure_args_min": 0, "function_args_max": 0, "function_args_min": 0},
                               "nexits": {"sum": 0, "average": 0.0,"min":0,"max":0},
                               "halstead": {"bugs": 0.000_942_552_557_372_941_4,
                                            "difficulty": 1.0,
                                            "effort": 4.754_887_502_163_468,
                                            "length": 3,
                                            "estimated_program_length": 2.0,
                                            "purity_ratio": 0.666_666_666_666_666_6,
                                            "level": 1.0,
                                            "total_operands": 1,
                                            "total_operators": 2,
                                            "vocabulary": 3,
                                            "time": 0.264_160_416_786_859_36,
                                            "unique_operands": 1,
                                            "unique_operators": 2,
                                            "volume": 4.754_887_502_163_468},
                               "loc": {"cloc": 0, "ploc": 2, "lloc": 1, "sloc": 2, "blank": 0, "cloc_average": 0.0, "ploc_average": 1.0, "lloc_average": 0.5, "sloc_average": 1.0, "blank_average": 0.0, "cloc_min": 0, "ploc_min": 2, "lloc_min": 1, "sloc_min": 2, "blank_min": 0, "cloc_max": 0, "ploc_max": 2, "lloc_max": 1, "sloc_max": 2, "blank_max": 0},
                               "nom": {"functions": 1, "closures": 0, "functions_average": 0.5, "closures_average": 0.0, "total": 1, "average": 0.5, "closures_min": 0, "closures_max": 0, "functions_min": 0, "functions_max": 1},
                               "tokens": {"tokens": 6, "tokens_average": 3.0, "tokens_min": 0, "tokens_max": 6},
                               "mi": {"original": 151.203_315_883_223_2,
                                      "sei": 142.643_061_717_489_76,
                                      "visual_studio": 88.422_991_744_574_97},
                               "abc": {"assignments": 0, "branches": 0, "conditions": 0, "magnitude": 0.0, "assignments_average": 0.0, "branches_average": 0.0, "conditions_average": 0.0, "assignments_min": 0, "assignments_max": 0, "branches_min": 0, "branches_max": 0, "conditions_min": 0, "conditions_max": 0},
                               "wmc": {"classes": 0, "interfaces": 0, "total": 0}},
                   "name": "test.py",
                   "spaces": [{"kind": "function",
                               "start_line": 1,
                               "end_line": 2,
                               "metrics": {"cyclomatic": {"sum": 1, "average": 1.0, "min": 1,"max": 1, "modified": {"sum": 1, "average": 1.0, "min": 1,"max": 1}},
                                           "cognitive": {"sum": 0, "average": 0.0 , "min":0, "max":0},
                                           "nargs": {"function_args": 0, "function_args_average": 0.0, "closure_args": 0, "closure_args_average": 0.0, "total": 0, "average": 0.0, "closure_args_max": 0, "closure_args_min": 0, "function_args_max": 0, "function_args_min": 0},
                                           "nexits": {"sum": 0, "average": 0.0,"min":0,"max":0},
                                           "halstead": {"bugs": 0.000_942_552_557_372_941_4,
                                                        "difficulty": 1.0,
                                                        "effort": 4.754_887_502_163_468,
                                                        "length": 3,
                                                        "estimated_program_length": 2.0,
                                                        "purity_ratio": 0.666_666_666_666_666_6,
                                                        "level": 1.0,
                                                        "total_operands": 1,
                                                        "total_operators": 2,
                                                        "vocabulary": 3,
                                                        "time": 0.264_160_416_786_859_36,
                                                        "unique_operands": 1,
                                                        "unique_operators": 2,
                                                        "volume": 4.754_887_502_163_468},
                                           "loc": {"cloc": 0, "ploc": 2, "lloc": 1, "sloc": 2, "blank": 0, "cloc_average": 0.0, "ploc_average": 2.0, "lloc_average": 1.0, "sloc_average": 2.0, "blank_average": 0.0, "cloc_min": 0, "ploc_min": 2, "lloc_min": 1, "sloc_min": 2, "blank_min": 0, "cloc_max": 0, "ploc_max": 2, "lloc_max": 1, "sloc_max": 2, "blank_max": 0},
                                           "nom": {"functions": 1, "closures": 0, "functions_average": 1.0, "closures_average": 0.0, "total": 1, "average": 1.0, "closures_min": 0, "closures_max": 0, "functions_min": 1, "functions_max": 1},
                                           "tokens": {"tokens": 6, "tokens_average": 6.0, "tokens_min": 6, "tokens_max": 6},
                                           "mi": {"original": 151.433_315_883_223_23,
                                                  "sei": 142.873_061_717_489_78,
                                                  "visual_studio": 88.557_494_668_551_6},
                                           "abc": {"assignments": 0, "branches": 0, "conditions": 0, "magnitude": 0.0, "assignments_average": 0.0, "branches_average": 0.0, "conditions_average": 0.0, "assignments_min": 0, "assignments_max": 0, "branches_min": 0, "branches_max": 0, "conditions_min": 0, "conditions_max": 0}},
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
        "language": "python",
        "spans": [
            {
                "end_line": 2,
                "name": "foo",
                "start_line": 1
            },
            {
                "end_line": 5,
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
        "language": "python",
        "spans": [
            {
                "end_line": 2,
                "name": "foo",
                "start_line": 1
            },
            {
                "end_line": 5,
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

#[traced_test]
#[actix_rt::test]
async fn test_run_parse_error_does_not_leak_internals() {
    let config = test_config();
    let result = run_parse(&config, "req-leak-json", || -> String {
        panic!("secret internal detail")
    })
    .await;
    assert_error_sanitized(result).await;
    // The server-side log must carry the correlation id, the failure
    // marker, and the underlying panic detail for ops diagnostics — even
    // though `assert_error_sanitized` already proved that same detail is
    // scrubbed from the HTTP body. Asserting the detail's presence in the
    // log and absence from the body on the same string pins the contract.
    assert!(logs_contain("Parse task failed"));
    assert!(logs_contain("req-leak-json"));
    assert!(logs_contain("secret internal detail"));
}

#[traced_test]
#[actix_rt::test]
async fn test_run_parse_error_with_timeout_does_not_leak_internals() {
    let config = test_config_with_timeout(Duration::from_secs(5));
    let result = run_parse(&config, "req-leak-timeout", || -> String {
        panic!("secret internal detail")
    })
    .await;
    assert_error_sanitized(result).await;
    assert!(logs_contain("Parse task failed"));
    assert!(logs_contain("req-leak-timeout"));
    assert!(logs_contain("secret internal detail"));
}

#[traced_test]
#[actix_rt::test]
async fn test_run_parse_timeout_returns_504() {
    let config = test_config_with_timeout(Duration::from_millis(50));
    // The blocking task outlives the timeout, but exits shortly after to keep
    // the test fast (the cleanup task awaits the JoinHandle).
    let result = run_parse(&config, "req-timeout", || {
        std::thread::sleep(Duration::from_millis(200));
        "completed"
    })
    .await;

    let err = result.unwrap_err();
    let resp = err.error_response();
    assert_eq!(resp.status(), StatusCode::GATEWAY_TIMEOUT);
    let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
    assert_eq!(String::from_utf8_lossy(&body), PARSE_TIMEOUT);
    // The timeout must be logged server-side, correlated to the request id.
    assert!(logs_contain("Parse timed out"));
    assert!(logs_contain("req-timeout"));
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

    let err = run_parse(&config, "req-orphan", move || {
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
    let result = run_parse(&config, "req-503", || "should not run").await;
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
                run_parse(&config_for_task, "req-recheck", move || {
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

// --- Content-Type guard interop (#515) ---------------------------------
//
// These tests build the app via the production `configure_routes` so they
// exercise the real `fn_guard` content-type matching and the diagnostic
// `default_service`, not a per-test inline route. A raw `Content-Type`
// header string is set explicitly (rather than `ContentType::json()`) so
// the guard sees exactly the bytes a real client would send.

/// A minimal C AST request body, reused across the guard tests.
fn ast_request_body() -> serde_json::Value {
    json!({
        "id": "ct-515",
        "file_name": "foo.c",
        "code": "int x = 1;",
        "comment": false,
        "span": true
    })
}

#[actix_rt::test]
async fn test_web_ast_accepts_json_charset_suffix() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    // The exact variant browsers / Python `requests` send and that the
    // old exact-match `guard::Header` rejected with a bodyless 404.
    let req = test::TestRequest::post()
        .uri("/ast")
        .insert_header(("content-type", "application/json; charset=utf-8"))
        .set_payload(ast_request_body().to_string())
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = test::read_body(resp).await;
    let parsed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["id"], json!("ct-515"));
    assert_eq!(parsed["root"]["type"], json!("translation_unit"));
}

#[actix_rt::test]
async fn test_web_ast_accepts_uppercase_content_type() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/ast")
        .insert_header(("content-type", "APPLICATION/JSON"))
        .set_payload(ast_request_body().to_string())
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = test::read_body(resp).await;
    let parsed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["id"], json!("ct-515"));
}

#[actix_rt::test]
async fn test_web_octet_stream_accepts_charset_suffix() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    // `application/octet-stream` with a parameter still matches the
    // octet-stream route by essence.
    let req = test::TestRequest::post()
        .uri("/comment?file_name=foo.c")
        .insert_header(("content-type", "application/octet-stream; boundary=x"))
        .set_payload("int x;//c")
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[actix_rt::test]
async fn test_web_missing_content_type_yields_415() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    // No content-type header at all on a known endpoint: the default
    // service must return a diagnosable 415, not a bodyless 404.
    let req = test::TestRequest::post()
        .uri("/ast")
        .set_payload(ast_request_body().to_string())
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let body = test::read_body(resp).await;
    let body_str = String::from_utf8_lossy(&body);
    assert!(
        body_str.contains("Content-Type"),
        "415 body must name the offending header: {body_str}"
    );
}

#[actix_rt::test]
async fn test_web_wrong_content_type_yields_415() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    // A media type matching neither json nor octet-stream on a known
    // endpoint is a 415, distinguishable from a wrong URL.
    let req = test::TestRequest::post()
        .uri("/metrics")
        .insert_header(("content-type", "text/plain"))
        .set_payload("int x = 1;")
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[actix_rt::test]
async fn test_web_unknown_url_still_404() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    // A genuinely unknown path remains a 404 even with a valid
    // content-type, so the 415 path does not swallow routing errors.
    let req = test::TestRequest::post()
        .uri("/does-not-exist")
        .insert_header(("content-type", "application/json"))
        .set_payload("{}")
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    // The app-level 404 now carries the uniform JSON `{error, id}` body
    // (#541) rather than the former bare `text/plain` "Not found".
    let body: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
    assert_eq!(body, json!({"error": "Not found", "id": ""}));
}

#[actix_rt::test]
async fn test_web_wrong_method_on_known_endpoint_yields_405() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    // A wrong HTTP method on a known POST-only endpoint is a method
    // error, not a content-type error: the default service must return
    // 405, distinguishable from the 415 a wrong/missing Content-Type
    // gets and the 404 an unknown URL gets.
    let req = test::TestRequest::get().uri("/metrics").to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    let body = test::read_body(resp).await;
    assert!(
        String::from_utf8_lossy(&body).contains("POST"),
        "405 body should name the accepted method"
    );
}

// --- /v1 versioned routes + deprecated unprefixed aliases (issue #517) ---
//
// Built via the production `configure_routes`, so both the `/v1` scope and
// the unprefixed aliases run through the real route table, content-type
// guards, and per-resource `default_service` fallbacks.

#[actix_rt::test]
async fn test_web_v1_ast_matches_unprefixed_alias() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    let payload = ast_request_body().to_string();
    let post_json = |uri: &'static str| {
        test::TestRequest::post()
            .uri(uri)
            .insert_header(ContentType::json())
            .set_payload(payload.clone())
            .to_request()
    };

    let v1_resp = test::call_service(&app, post_json("/v1/ast")).await;
    assert_eq!(v1_resp.status(), StatusCode::OK);
    let v1_body: Value = serde_json::from_slice(&test::read_body(v1_resp).await).unwrap();

    let alias_resp = test::call_service(&app, post_json("/ast")).await;
    assert_eq!(alias_resp.status(), StatusCode::OK);
    let alias_body: Value = serde_json::from_slice(&test::read_body(alias_resp).await).unwrap();

    // The versioned route and the deprecated alias must be byte-identical.
    assert_eq!(v1_body, alias_body);
    assert_eq!(v1_body["root"]["type"], json!("translation_unit"));
}

#[actix_rt::test]
async fn test_web_v1_post_endpoints_return_200() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // file_name selects the language; bodies are minimal but valid. The
    // distinct `id` per case lets us confirm the handler actually ran and
    // echoed our request, not just that *some* 200 was produced.
    let cases = [
        (
            "/v1/metrics",
            json!({"id": "id-metrics", "file_name": "a.c", "code": "int x = 1;", "unit": false}),
        ),
        (
            "/v1/comment",
            json!({"id": "id-comment", "file_name": "a.c", "code": "int x = 1; // hi"}),
        ),
        (
            "/v1/function",
            json!({"id": "id-function", "file_name": "a.c", "code": "int f() { return 0; }"}),
        ),
    ];
    for (uri, body) in cases {
        let expected_id = body["id"].clone();
        let req = test::TestRequest::post()
            .uri(uri)
            .insert_header(ContentType::json())
            .set_payload(body.to_string())
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK, "{uri} should return 200");
        let parsed: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
        assert_eq!(
            parsed["id"], expected_id,
            "{uri} should echo the request id"
        );
    }
}

#[actix_rt::test]
async fn test_web_v1_metrics_spaces_is_present_object() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // `spaces` is now a non-optional `FuncSpace` (#517): a successful
    // response carries the unit-space object directly, never `null`.
    let req = test::TestRequest::post()
        .uri("/v1/metrics")
        .insert_header(ContentType::json())
        .set_payload(
            json!({"id": "m", "file_name": "a.c", "code": "int x = 1;", "unit": false}).to_string(),
        )
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
    assert_eq!(body["spaces"]["kind"], json!("unit"));
}

#[actix_rt::test]
async fn test_web_v1_ping_works_and_unprefixed_alias_too() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    for uri in ["/v1/ping", "/ping"] {
        let resp = test::call_service(&app, test::TestRequest::get().uri(uri).to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK, "GET {uri} should return 200");
    }
}

#[actix_rt::test]
async fn test_web_post_to_ping_yields_405() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // `/ping` is GET-only; its own `default_service` answers a method
    // error (not the 415 a content-type-guarded POST endpoint gives, nor
    // the bodyless 404 the pre-#517 path constant produced for `/ping`).
    for uri in ["/ping", "/v1/ping"] {
        let resp = test::call_service(&app, test::TestRequest::post().uri(uri).to_request()).await;
        assert_eq!(
            resp.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "POST {uri} should return 405"
        );
        let body = test::read_body(resp).await;
        assert!(
            String::from_utf8_lossy(&body).contains("GET"),
            "405 body should name the accepted method"
        );
    }
}

#[actix_rt::test]
async fn test_web_v1_unknown_url_still_404() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    let req = test::TestRequest::post()
        .uri("/v1/does-not-exist")
        .insert_header(ContentType::json())
        .set_payload("{}")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[actix_rt::test]
async fn test_web_v1_wrong_content_type_yields_415() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    let req = test::TestRequest::post()
        .uri("/v1/metrics")
        .insert_header(("content-type", "text/plain"))
        .set_payload("int x = 1;")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[actix_rt::test]
async fn test_web_v1_wrong_method_yields_405() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    let resp = test::call_service(
        &app,
        test::TestRequest::get().uri("/v1/metrics").to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    let body = test::read_body(resp).await;
    assert!(
        String::from_utf8_lossy(&body).contains("POST"),
        "405 body should name the accepted method"
    );
}

// --- Uniform JSON error body across every endpoint (issue #541) ---------
//
// The core regression guard: an error on a JSON endpoint, an
// octet-stream endpoint, a 415 (bad content-type) and a 405 (bad
// method) must ALL return a body parseable as `{error, id}` with the
// right status, so clients parse one error shape regardless of the
// success content-type.

/// Asserts `body` parses as the uniform `{error, id}` shape with a
/// non-empty `error` string and `id == expected_id`.
fn assert_uniform_error_body(body: &[u8], expected_id: &str) {
    let parsed: Value =
        serde_json::from_slice(body).expect("error body must be valid JSON `{error, id}`");
    assert!(
        parsed["error"].as_str().is_some_and(|s| !s.is_empty()),
        "error body must carry a non-empty `error` string: {parsed}"
    );
    assert_eq!(
        parsed["id"],
        json!(expected_id),
        "error body must echo the request id (empty when absent): {parsed}"
    );
}

#[actix_rt::test]
async fn test_web_error_body_uniform_across_endpoints() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // 1) JSON endpoint, invalid language -> 404 JSON `{error, id}` with
    //    the echoed id.
    let json_req = test::TestRequest::post()
        .uri("/v1/metrics")
        .insert_header(ContentType::json())
        .set_payload(
            json!({"id": "err-json", "file_name": "x.unknown_ext", "code": "x", "unit": false})
                .to_string(),
        )
        .to_request();
    let json_resp = test::call_service(&app, json_req).await;
    assert_eq!(json_resp.status(), StatusCode::NOT_FOUND);
    assert_uniform_error_body(&test::read_body(json_resp).await, "err-json");

    // 2) Octet-stream endpoint, invalid language -> 404 JSON `{error,
    //    id}` (formerly a bare `text/plain` "error: ..." body). The
    //    octet-stream variants carry no id, so `id` is the empty string.
    let octet_req = test::TestRequest::post()
        .uri("/v1/metrics?file_name=x.unknown_ext")
        .insert_header(ContentType::octet_stream())
        .set_payload("int x = 1;")
        .to_request();
    let octet_resp = test::call_service(&app, octet_req).await;
    assert_eq!(octet_resp.status(), StatusCode::NOT_FOUND);
    assert_uniform_error_body(&test::read_body(octet_resp).await, "");

    // 3) 415: a known endpoint with an unsupported content-type.
    let unsupported_req = test::TestRequest::post()
        .uri("/v1/metrics")
        .insert_header(("content-type", "text/plain"))
        .set_payload("int x = 1;")
        .to_request();
    let unsupported_resp = test::call_service(&app, unsupported_req).await;
    assert_eq!(
        unsupported_resp.status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert_uniform_error_body(&test::read_body(unsupported_resp).await, "");

    // 4) 405: a wrong method on a POST-only endpoint.
    let method_resp = test::call_service(
        &app,
        test::TestRequest::get().uri("/v1/metrics").to_request(),
    )
    .await;
    assert_eq!(method_resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_uniform_error_body(&test::read_body(method_resp).await, "");
}

// --- Success envelope carries id + canonical language slug (#541) -------

#[actix_rt::test]
async fn test_web_function_envelope_carries_id_and_language_slug() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    // A `.cpp` file must report the #540 canonical slug `cpp`, plus the
    // echoed id and the `spans` result key.
    let req = test::TestRequest::post()
        .uri("/v1/function")
        .insert_header(ContentType::json())
        .set_payload(
            json!({"id": "fn-env", "file_name": "a.cpp", "code": "int f() { return 0; }"})
                .to_string(),
        )
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
    assert_eq!(body["id"], json!("fn-env"));
    assert_eq!(body["language"], json!("cpp"));
    assert!(body["spans"].is_array(), "function envelope keeps `spans`");
}

#[actix_rt::test]
async fn test_web_comment_envelope_carries_id_and_language_slug() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/v1/comment")
        .insert_header(ContentType::json())
        .set_payload(
            json!({"id": "cm-env", "file_name": "a.cpp", "code": "int x = 1; // c"}).to_string(),
        )
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
    assert_eq!(body["id"], json!("cm-env"));
    // Reports the *guessed* language slug (`cpp`), not the internal
    // `ccomment` grammar the comment-removal path swaps in.
    assert_eq!(body["language"], json!("cpp"));
    assert!(
        body.get("code").is_some(),
        "comment envelope keeps the `code` result key"
    );
}

// --- `unit` query flag: normal bool semantics (#541) --------------------

#[actix_rt::test]
async fn test_web_metrics_plain_unit_flag_accepts_bool_forms() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // `true`/`1` (and case variants) enable unit-only metrics: the
    // response carries no nested `spaces`. `false`/`0`/absent keep the
    // full tree, which for this single-function source has a non-empty
    // nested `spaces` array.
    let post_unit = |unit: Option<&str>| {
        let uri = match unit {
            Some(v) => format!("/v1/metrics?file_name=u.py&unit={v}"),
            None => "/v1/metrics?file_name=u.py".to_string(),
        };
        test::TestRequest::post()
            .uri(&uri)
            .insert_header(ContentType::octet_stream())
            .set_payload("def foo():\n    pass\n")
            .to_request()
    };

    for truthy in ["true", "TRUE", "True", "1"] {
        let resp = test::call_service(&app, post_unit(Some(truthy))).await;
        assert_eq!(resp.status(), StatusCode::OK, "unit={truthy} should be 200");
        let body: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
        assert_eq!(
            body["spaces"]["spaces"],
            json!([]),
            "unit={truthy} should clear nested spaces"
        );
    }

    for falsy in ["false", "FALSE", "0"] {
        let resp = test::call_service(&app, post_unit(Some(falsy))).await;
        assert_eq!(resp.status(), StatusCode::OK, "unit={falsy} should be 200");
        let body: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
        assert!(
            body["spaces"]["spaces"]
                .as_array()
                .is_some_and(|s| !s.is_empty()),
            "unit={falsy} should keep nested spaces"
        );
    }

    // Absent defaults to false (full tree).
    let resp = test::call_service(&app, post_unit(None)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
    assert!(
        body["spaces"]["spaces"]
            .as_array()
            .is_some_and(|s| !s.is_empty())
    );
}

#[actix_rt::test]
async fn test_web_metrics_plain_unit_flag_rejects_non_bool() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // The former lenient truthy set (`yes`/`on`) is gone (#541); any
    // value that is not a recognised bool is a 400 with the uniform
    // JSON error body.
    for bad in ["yes", "on", "bogus"] {
        let req = test::TestRequest::post()
            .uri(&format!("/v1/metrics?file_name=u.py&unit={bad}"))
            .insert_header(ContentType::octet_stream())
            .set_payload("def foo():\n    pass\n")
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "unit={bad} should be rejected with 400"
        );
        assert_uniform_error_body(&test::read_body(resp).await, "");
    }
}

// --- Introspection endpoints + unprefixed aliases (#541) ----------------

#[actix_rt::test]
async fn test_web_version_endpoint_reports_server_and_library() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    for uri in ["/v1/version", "/version"] {
        let resp = test::call_service(&app, test::TestRequest::get().uri(uri).to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK, "GET {uri} should return 200");
        let body: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
        assert_eq!(
            body["server"],
            json!(env!("CARGO_PKG_VERSION")),
            "{uri} should report the server crate version"
        );
        assert_eq!(
            body["library"],
            json!(big_code_analysis::VERSION),
            "{uri} should report the library version"
        );
    }
}

#[actix_rt::test]
async fn test_web_languages_endpoint_lists_slugs_and_extensions() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    for uri in ["/v1/languages", "/languages"] {
        let resp = test::call_service(&app, test::TestRequest::get().uri(uri).to_request()).await;
        assert_eq!(resp.status(), StatusCode::OK, "GET {uri} should return 200");
        let body: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
        let languages = body["languages"]
            .as_array()
            .expect("`languages` must be an array");
        assert!(!languages.is_empty(), "language list must not be empty");

        // A known language reported by its #540 slug, with a known
        // extension present in its extension list.
        let cpp = languages
            .iter()
            .find(|entry| entry["name"] == json!("cpp"))
            .expect("`cpp` must be listed by its canonical slug");
        let exts: Vec<&str> = cpp["extensions"]
            .as_array()
            .expect("extensions must be an array")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(
            exts.contains(&"cpp"),
            "cpp extensions should include `cpp`: {exts:?}"
        );
    }
}

#[actix_rt::test]
async fn test_web_introspection_endpoints_reject_post_with_405() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // The introspection resources are GET-only; a POST must hit their
    // own `default_service` and answer a uniform JSON 405.
    for uri in ["/v1/version", "/v1/languages"] {
        let resp = test::call_service(&app, test::TestRequest::post().uri(uri).to_request()).await;
        assert_eq!(
            resp.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "POST {uri} should return 405"
        );
        assert_uniform_error_body(&test::read_body(resp).await, "");
    }
}

// --- POST /vcs (issue #328) ----------------------------------------------

/// Build a throwaway git repo with one file committed ~5 days ago
/// (within both default windows). Commit dates are relative to wall
/// clock because `/vcs` uses wall-clock `now` by default.
fn build_temp_repo() -> tempfile::TempDir {
    use std::process::Command as StdCommand;
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after epoch")
        .as_secs();
    let date = format!("@{} +0000", now - 5 * 86_400);
    let dir = tempfile::tempdir().expect("tempdir");
    let git = |args: &[&str]| {
        let ok = StdCommand::new("git")
            .args(args)
            .current_dir(dir.path())
            .env("GIT_AUTHOR_NAME", "Ada")
            .env("GIT_AUTHOR_EMAIL", "ada@example.com")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_NAME", "Ada")
            .env("GIT_COMMITTER_EMAIL", "ada@example.com")
            .env("GIT_COMMITTER_DATE", &date)
            .status()
            .expect("spawn git")
            .success();
        assert!(ok, "git {args:?} failed");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "commit.gpgsign", "false"]);
    std::fs::write(dir.path().join("work.rs"), "fn a() {}\n").expect("write");
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "add work"]);
    dir
}

#[actix_rt::test]
async fn test_web_vcs_ranks_files() {
    let repo = build_temp_repo();
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .service(web::resource("/vcs").route(web::post().to(vcs_json))),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/vcs")
        .insert_header(ContentType::json())
        .set_json(json!({ "id": "req-1", "repo_path": repo.path().to_str().unwrap() }))
        .to_request();
    let res: Value = test::call_and_read_body_json(&app, req).await;
    assert_eq!(res["id"], "req-1");
    assert_eq!(res["long_window_days"], 365);
    let files = res["files"].as_array().expect("files array");
    let work = files
        .iter()
        .find(|f| f["path"] == "work.rs")
        .expect("work.rs ranked");
    assert_eq!(work["commits_long"], 1);
    assert_eq!(work["commits_recent"], 1);
    // The bus-factor aggregate ships alongside the ranked files (#332):
    // one solo author over one file ⇒ repo bus factor 1.
    let bus_factor = &res["vcs_aggregate"]["bus_factor"];
    assert_eq!(bus_factor["repo"]["bus_factor"], 1);
    assert_eq!(bus_factor["repo"]["files"], 1);
    assert_eq!(bus_factor["schema_version"], 1);
}

#[actix_rt::test]
async fn test_web_vcs_bad_bus_factor_threshold_is_400() {
    // A threshold outside (0, 1) is a client mistake, so the new
    // `InvalidBusFactorThreshold` error must map to 400, not 500 (#332).
    let repo = build_temp_repo();
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .service(web::resource("/vcs").route(web::post().to(vcs_json))),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/vcs")
        .insert_header(ContentType::json())
        .set_json(json!({
            "id": "req-bf",
            "repo_path": repo.path().to_str().unwrap(),
            "bus_factor_threshold": 1.5,
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[actix_rt::test]
async fn test_web_vcs_bad_file_types_is_400() {
    // An unparseable file-type scope is a client mistake, so the new
    // `InvalidFileTypeScope` error must map to 400, not 500 (#576) — the
    // same class of regression `test_web_vcs_bad_bus_factor_threshold_is_400`
    // guards for #332. An empty string normalises to no scope.
    let repo = build_temp_repo();
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .service(web::resource("/vcs").route(web::post().to(vcs_json))),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/vcs")
        .insert_header(ContentType::json())
        .set_json(json!({
            "id": "req-ft",
            "repo_path": repo.path().to_str().unwrap(),
            "file_types": "",
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[actix_rt::test]
async fn test_web_vcs_outside_repo_is_400() {
    let dir = tempfile::tempdir().expect("tempdir");
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .service(web::resource("/vcs").route(web::post().to(vcs_json))),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/vcs")
        .insert_header(ContentType::json())
        .set_json(json!({ "id": "req-2", "repo_path": dir.path().to_str().unwrap() }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[actix_rt::test]
async fn test_web_vcs_wrong_content_type_yields_415() {
    // `/vcs` must carry the same per-resource fallback as the other POST
    // endpoints (#515): a wrong Content-Type is a diagnostic 415, not a
    // bodyless app-level 404.
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/vcs")
        .insert_header(("content-type", "text/plain"))
        .set_payload("not json")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "/vcs wrong Content-Type must be a 415, not a 404"
    );
}

#[actix_rt::test]
async fn test_web_vcs_wrong_method_yields_405() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let req = test::TestRequest::get().uri("/vcs").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::METHOD_NOT_ALLOWED,
        "GET /vcs must be a 405, not a 404"
    );
    // The body proves the response came from our guarded_post_fallback
    // (which names the accepted method), not actix's built-in 405 — i.e.
    // the per-resource default_service is actually wired.
    let body = test::read_body(resp).await;
    assert!(
        String::from_utf8_lossy(&body).contains("POST"),
        "405 body should name the accepted method"
    );
}

/// Build a deterministic two-commit git repo for the `/vcs/trend` tests:
/// `early.rs` at `now − 300d`, `late.rs` added at `now − 100d`. Returns
/// the tempdir (auto-removed on drop).
fn build_trend_repo() -> tempfile::TempDir {
    use std::process::Command;
    const FIXED_NOW: i64 = 1_700_000_000;
    const DAY: i64 = 86_400;

    let dir = tempfile::tempdir().expect("tempdir");
    let run = |args: &[&str], date: Option<i64>| {
        let mut cmd = Command::new("git");
        cmd.args(args).current_dir(dir.path());
        if let Some(secs) = date {
            let d = format!("@{secs} +0000");
            cmd.envs([
                ("GIT_AUTHOR_NAME", "Ada"),
                ("GIT_AUTHOR_EMAIL", "ada@example.com"),
                ("GIT_AUTHOR_DATE", d.as_str()),
                ("GIT_COMMITTER_NAME", "Ada"),
                ("GIT_COMMITTER_EMAIL", "ada@example.com"),
                ("GIT_COMMITTER_DATE", d.as_str()),
            ]);
        }
        assert!(cmd.status().expect("spawn git").success(), "git {args:?}");
    };
    run(&["init", "-q", "-b", "main"], None);
    run(&["config", "commit.gpgsign", "false"], None);
    std::fs::write(dir.path().join("early.rs"), "fn a() {}\n").expect("write");
    run(&["add", "-A"], None);
    run(
        &["commit", "-q", "--no-verify", "-m", "early"],
        Some(FIXED_NOW - 300 * DAY),
    );
    std::fs::write(dir.path().join("late.rs"), "fn b() {}\n").expect("write");
    run(&["add", "-A"], None);
    run(
        &["commit", "-q", "--no-verify", "-m", "late"],
        Some(FIXED_NOW - 100 * DAY),
    );
    dir
}

#[actix_rt::test]
async fn test_web_vcs_trend_json_shape() {
    let repo = build_trend_repo();
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let payload = json!({
        "id": "trend-1",
        "repo_path": repo.path().to_str().unwrap(),
        "as_of": "@1700000000",
        "points": 3,
        "span": "300d",
    });
    let req = test::TestRequest::post()
        .uri("/vcs/trend")
        .insert_header(ContentType::json())
        .set_json(&payload)
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body["id"], "trend-1");
    assert_eq!(body["trend_schema_version"], 1);
    assert_eq!(body["as_of_points"].as_array().unwrap().len(), 3);
    // late.rs is absent at the oldest point (it was added later).
    let late = body["files"]["late.rs"].as_array().expect("late.rs series");
    assert!(late[0].is_null(), "late.rs is null at the oldest point");
    assert!(late[2].is_object(), "late.rs present at the newest point");
}

#[actix_rt::test]
async fn test_web_vcs_trend_too_few_points_is_400() {
    let repo = build_trend_repo();
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let payload = json!({
        "id": "trend-bad",
        "repo_path": repo.path().to_str().unwrap(),
        "points": 1,
        "span": "300d",
    });
    let req = test::TestRequest::post()
        .uri("/vcs/trend")
        .insert_header(ContentType::json())
        .set_json(&payload)
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a sub-minimum point count is a client error"
    );
}
