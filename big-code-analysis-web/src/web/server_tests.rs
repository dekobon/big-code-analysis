// Sibling-file integration-style tests for the `web::server` routes
// and helpers, wired in via `#[path = "server_tests.rs"] mod tests;`
// so the production `server.rs` stays under the `bca check` per-file
// metric caps. The `./**/*_tests.rs` rule in `.bcaignore` keeps this
// file out of the self-scan walker.

use actix_web::web::Bytes;
use actix_web::{ResponseError as _, http::StatusCode, http::header::ContentType, test};
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

async fn assert_error_sanitized(result: Result<String, ParseError>) {
    let err = result.unwrap_err();
    let resp = err.error_response();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    // The 500 now carries the uniform `{error, id}` JSON body (#639), not
    // a bare `text/plain` string.
    assert_eq!(
        resp.headers().get(http::header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
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
    let parsed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["error"], json!("Internal server error"));
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
    // flat named object `{start_line, start_col, end_line, end_col}`
    // (#535); the four values preserve the former tuple order.
    let span =
        |sr, sc, er, ec| json!({"start_line": sr, "start_col": sc, "end_line": er, "end_col": ec});
    let expected = json!({
        "id": "1234",
        // The /ast envelope echoes the resolved language slug, matching the
        // other analysis endpoints (#654). Since #721 `.c` resolves to the
        // dedicated C grammar (`LANG::C`, slug `c`); C++ headers and
        // sources stay on `LANG::Cpp`.
        "language": "c",
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
        |sr, sc, er, ec| json!({"start_line": sr, "start_col": sc, "end_line": er, "end_col": ec});
    let expected = json!({
        "id": "1234",
        // /ast echoes the resolved language slug (#654); `foo.js` is
        // upstream `javascript` (#507).
        "language": "javascript",
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
    // The JSON variant returns `code` as a string equal to the stripped
    // source — not a serde byte array (#629). The comment is gone and the
    // declaration survives, so assert both presence and absence.
    let expected = json!({
        "id": "1234",
        "language": "c",
        "code": "int x = 1; ",
    });

    assert_eq!(res, expected);
    let code = res["code"].as_str().expect("code is a JSON string");
    assert!(code.contains("int x = 1;"), "declaration survives");
    assert!(!code.contains("hello"), "comment removed");
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

    // Unknown extension is an unprocessable entity, not a missing route:
    // the route matched and the body parsed (issue #634).
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let res: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
    let expected = json!({
        "id": "1234",
        "error": UNSUPPORTED_LANGUAGE,
        "error_kind": "unsupported_language",
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

    // No comment in the code, so `code` is the empty string — the success
    // envelope stays uniform with a non-empty result (#558) and the JSON
    // variant reports it as a string, not a byte array (#629).
    let expected = json!({
        "id": "1234",
        "language": "c",
        "code": "",
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
    // Unknown extension -> 422, not 404: the route matched (issue #634).
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // Errors on the octet-stream endpoint now use the uniform JSON
    // `{error, id}` body (#541), not a bare `text/plain` string.
    let body: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
    let expected = json!({
        "error": UNSUPPORTED_LANGUAGE,
        "error_kind": "unsupported_language",
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
/// a uniform empty payload (JSON `{code: ""}`, octet-stream empty body).
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
    // key is the empty string (#629); the octet-stream body is empty bytes.
    assert_eq!(
        json_body,
        json!({ "id": "1234", "language": "c", "code": "" })
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
            scope: Scope::Full,
        })
        .to_request();

    let res: Value = test::call_and_read_body_json(&app, req).await;
    // The single root space sits under `root` (not the misleading plural
    // `spaces`); its own nested `spaces` list holds the children (#638).
    let expected = json!({
        "id": "1234",
        "language": "python",
        "root": {"kind": "unit",
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
                               "tokens": {"tokens": 6, "average": 3.0, "min": 0, "max": 6},
                               "mi": {"original": 139.974_331_558_152_1,
                                      "sei": 176.151_664_509_301_33,
                                      "visual_studio": 81.856_334_244_533_39},
                               "abc": {"assignments": 0, "branches": 0, "conditions": 0, "magnitude": 0.0, "assignments_average": 0.0, "branches_average": 0.0, "conditions_average": 0.0, "assignments_min": 0, "assignments_max": 0, "branches_min": 0, "branches_max": 0, "conditions_min": 0, "conditions_max": 0},
                               "wmc": {"class_wmc_sum": 0, "interface_wmc_sum": 0, "total": 0}},
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
                                           "tokens": {"tokens": 6, "average": 6.0, "min": 6, "max": 6},
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
            scope: Scope::File,
        })
        .to_request();

    let res: Value = test::call_and_read_body_json(&app, req).await;
    // `scope: File` returns only the file-level root (no nested children),
    // emitted under the `root` envelope key (#638).
    let expected = json!({
        "id": "1234",
        "language": "python",
        "root": {"kind": "unit",
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
                               "tokens": {"tokens": 6, "average": 3.0, "min": 0, "max": 6},
                               "mi": {"original": 151.203_315_883_223_2,
                                      "sei": 142.643_061_717_489_76,
                                      "visual_studio": 88.422_991_744_574_97},
                               "abc": {"assignments": 0, "branches": 0, "conditions": 0, "magnitude": 0.0, "assignments_average": 0.0, "branches_average": 0.0, "conditions_average": 0.0, "assignments_min": 0, "assignments_max": 0, "branches_min": 0, "branches_max": 0, "conditions_min": 0, "conditions_max": 0},
                               "wmc": {"class_wmc_sum": 0, "interface_wmc_sum": 0, "total": 0}},
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
                scope: Scope::Full,
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
    // Default scope (`full`) returns the nested tree under the `root`
    // envelope key (#638).
    let expected = json!({
        "id": "",
        "language": "python",
        "root": {"kind": "unit",
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
                               "tokens": {"tokens": 6, "average": 3.0, "min": 0, "max": 6},
                               "mi": {"original": 151.203_315_883_223_2,
                                      "sei": 142.643_061_717_489_76,
                                      "visual_studio": 88.422_991_744_574_97},
                               "abc": {"assignments": 0, "branches": 0, "conditions": 0, "magnitude": 0.0, "assignments_average": 0.0, "branches_average": 0.0, "conditions_average": 0.0, "assignments_min": 0, "assignments_max": 0, "branches_min": 0, "branches_max": 0, "conditions_min": 0, "conditions_max": 0},
                               "wmc": {"class_wmc_sum": 0, "interface_wmc_sum": 0, "total": 0}},
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
                                           "tokens": {"tokens": 6, "average": 6.0, "min": 6, "max": 6},
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
    // The 504 now carries the uniform `{error, id}` JSON body (#639); the
    // `id` echoes the correlation id passed to `run_parse`.
    assert_eq!(
        resp.headers().get(http::header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
    let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
    let parsed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["error"], json!(PARSE_TIMEOUT));
    assert_eq!(parsed["id"], json!("req-timeout"));
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
    // 503 now carries the uniform `{error, id}` JSON body (#639).
    let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
    let parsed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["error"], json!("parse pool saturated"));
    assert_eq!(parsed["id"], json!("req-503"));
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
    // 503 now carries the uniform `{error, id}` JSON body (#639).
    let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
    let parsed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["error"], json!("parse pool saturated"));
    assert_eq!(parsed["id"], json!("req-recheck"));
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
        .uri("/v1/ast")
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
        .uri("/v1/ast")
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
        .uri("/v1/comment?file_name=foo.c")
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
        .uri("/v1/ast")
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
        .uri("/v1/metrics")
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
    assert_eq!(
        body,
        json!({"error": "Not found", "error_kind": "not_found", "id": ""})
    );
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
    let req = test::TestRequest::default()
        .method(http::Method::PUT)
        .uri("/v1/metrics")
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    // RFC 9110 §15.5.6: a 405 MUST advertise the supported methods (#655).
    assert_eq!(
        resp.headers().get(http::header::ALLOW).unwrap(),
        "POST, OPTIONS",
        "405 must carry an Allow header naming the supported methods"
    );
    let body = test::read_body(resp).await;
    assert!(
        String::from_utf8_lossy(&body).contains("POST"),
        "405 body should name the accepted method"
    );
}

#[actix_rt::test]
async fn test_web_options_on_post_endpoint_yields_204_with_allow() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // OPTIONS is the method-discovery verb: a POST-only resource answers
    // 204 No Content advertising its methods so clients can discover them
    // without triggering a 405 (#655).
    let req = test::TestRequest::default()
        .method(http::Method::OPTIONS)
        .uri("/v1/metrics")
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        resp.headers().get(http::header::ALLOW).unwrap(),
        "POST, OPTIONS",
        "OPTIONS must advertise the resource's supported methods"
    );
    let body = test::read_body(resp).await;
    assert!(body.is_empty(), "204 response must have no body");
}

#[actix_rt::test]
async fn test_web_wrong_method_on_get_endpoint_yields_405_with_allow() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // A non-GET method on a GET-only resource is a 405 that MUST advertise
    // the supported methods, including the HEAD served alongside GET (#655).
    let req = test::TestRequest::post().uri("/v1/ping").to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        resp.headers().get(http::header::ALLOW).unwrap(),
        "GET, HEAD, OPTIONS",
        "405 on a GET-only resource must advertise GET, HEAD, OPTIONS"
    );
    let body = test::read_body(resp).await;
    assert!(
        String::from_utf8_lossy(&body).contains("GET"),
        "405 body should name the accepted method"
    );
}

#[actix_rt::test]
async fn test_web_options_on_get_endpoint_yields_204_with_allow() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // OPTIONS on a GET-only resource answers 204 advertising the full
    // method set (GET, HEAD, OPTIONS) for client method discovery (#655).
    let req = test::TestRequest::default()
        .method(http::Method::OPTIONS)
        .uri("/v1/ping")
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        resp.headers().get(http::header::ALLOW).unwrap(),
        "GET, HEAD, OPTIONS",
        "OPTIONS on a GET-only resource must advertise GET, HEAD, OPTIONS"
    );
    let body = test::read_body(resp).await;
    assert!(body.is_empty(), "204 response must have no body");
}

#[actix_rt::test]
async fn test_web_head_on_get_endpoints_matches_get() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // RFC 9110 §9.3.2: HEAD must be answered wherever GET is supported,
    // returning the same status and headers as GET but no body (#644).
    // Uptime monitors and load balancers default to HEAD probes against
    // `/ping`, so this is the endpoint's primary use case.
    //
    // #647 forcing function: the probed paths are *derived* from the
    // `ROUTES` index — every route advertising `GET` — rather than a
    // hand-maintained list. A future GET route added to `ROUTES` (and so to
    // `register_endpoints`) is automatically HEAD-probed here; if it was
    // registered with a bare `web::get()` instead of the `get_resource`
    // helper, this test goes red on the missing HEAD. `/v1` (the index
    // itself) is now covered too, which the old hardcoded list omitted.
    let get_paths: Vec<&str> = ROUTES
        .iter()
        .filter(|entry| entry.methods.contains(&"GET"))
        .map(|entry| entry.path)
        .collect();
    assert!(
        get_paths.len() >= 4,
        "expected at least the four introspection GET routes, got {get_paths:?}"
    );
    for path in get_paths {
        let get_resp =
            test::call_service(&app, test::TestRequest::get().uri(path).to_request()).await;
        let get_status = get_resp.status();
        let get_content_type = get_resp
            .headers()
            .get("content-type")
            .map(|v| v.to_str().unwrap().to_owned());
        assert_eq!(get_status, StatusCode::OK, "GET {path} should be 200");
        let get_body = test::read_body(get_resp).await;

        let head_resp = test::call_service(
            &app,
            test::TestRequest::default()
                .method(http::Method::HEAD)
                .uri(path)
                .to_request(),
        )
        .await;
        assert_eq!(
            head_resp.status(),
            get_status,
            "HEAD {path} status must match GET",
        );
        assert_eq!(
            head_resp
                .headers()
                .get("content-type")
                .map(|v| v.to_str().unwrap().to_owned()),
            get_content_type,
            "HEAD {path} Content-Type must match GET",
        );
        // The GET handler is reached for HEAD, so the in-memory test
        // service returns the same body. On the wire actix's HTTP codec
        // drops the body for HEAD requests; `test::call_service` bypasses
        // that codec, so body-stripping is not observable here. Asserting
        // the body matches GET pins that HEAD shares the GET code path
        // (the regression was a 405 fallback, not a body mismatch).
        let head_body = test::read_body(head_resp).await;
        assert_eq!(
            head_body, get_body,
            "HEAD {path} must reach the GET handler and produce its body",
        );
    }
}

#[actix_rt::test]
async fn test_web_head_on_post_endpoint_still_405() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // HEAD is only delegated to GET handlers; a POST-only endpoint must
    // still answer 405 naming the accepted method, so the GET-or-HEAD
    // guard does not accidentally widen the POST resources (#644).
    let req = test::TestRequest::default()
        .method(http::Method::HEAD)
        .uri("/v1/metrics")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    let body = test::read_body(resp).await;
    assert!(
        String::from_utf8_lossy(&body).contains("POST"),
        "405 body should name the accepted method",
    );
}

// --- /v1 versioned routes; unprefixed aliases removed at 2.0 (#637) ------
//
// Built via the production `configure_routes`, so the `/v1` scope runs
// through the real route table, content-type guards, and per-resource
// `default_service` fallbacks. The unprefixed aliases were removed at 2.0
// (#637); the tests below pin that every bare path now 404s.

#[actix_rt::test]
async fn test_web_v1_ast_returns_expected_envelope() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    let payload = ast_request_body().to_string();
    let v1_resp = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/v1/ast")
            .insert_header(ContentType::json())
            .set_payload(payload)
            .to_request(),
    )
    .await;
    assert_eq!(v1_resp.status(), StatusCode::OK);
    let v1_body: Value = serde_json::from_slice(&test::read_body(v1_resp).await).unwrap();
    assert_eq!(v1_body["root"]["type"], json!("translation_unit"));
}

/// The unprefixed aliases were removed at 2.0 (#637): every bare path now
/// falls through to the app-level `404`, while its `/v1` twin still works.
#[actix_rt::test]
async fn test_web_unprefixed_routes_now_404() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // The full former-alias surface mirrored from `register_endpoints` plus
    // the bare `/` index alias.
    let alias_paths = [
        "/",
        "/ast",
        "/comment",
        "/metrics",
        "/vcs",
        "/vcs/trend",
        "/vcs/jit",
        "/function",
        "/ping",
        "/version",
        "/languages",
    ];

    for path in alias_paths {
        // A bare path now matches no registered resource and 404s with the
        // uniform `{error, error_kind, id}` body — no deprecation headers,
        // because the alias scope is gone.
        let resp = test::call_service(&app, test::TestRequest::get().uri(path).to_request()).await;
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "unprefixed {path} must 404 now the alias is removed (#637)",
        );
        let headers_have_deprecation = resp.headers().contains_key("deprecation");
        let body = test::read_body(resp).await;
        assert!(
            !headers_have_deprecation,
            "unprefixed {path} must not carry a Deprecation header (alias removed)",
        );
        assert_uniform_error_body(&body, "");
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            parsed["error_kind"],
            json!("not_found"),
            "removed alias {path} must carry the not_found token",
        );
    }

    // The canonical `/v1` POST twin still works.
    let body = json!({"id": "v1", "file_name": "a.c", "code": "int x = 1;"}).to_string();
    let v1 = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/v1/metrics")
            .insert_header(ContentType::json())
            .set_payload(body)
            .to_request(),
    )
    .await;
    assert_eq!(v1.status(), StatusCode::OK, "the /v1 twin must still work");
    // And the /v1 routes never carried the deprecation headers.
    assert!(
        !v1.headers().contains_key("deprecation"),
        "/v1 routes must not carry a Deprecation header",
    );
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
            json!({"id": "id-metrics", "file_name": "a.c", "code": "int x = 1;"}),
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
async fn test_web_v1_metrics_root_is_present_object() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // `root` is a non-optional `FuncSpace` (#517) under the `root` envelope
    // key (#638): a successful response carries the unit-space object
    // directly, never `null`, and never under the misleading plural
    // `spaces`.
    let req = test::TestRequest::post()
        .uri("/v1/metrics")
        .insert_header(ContentType::json())
        .set_payload(json!({"id": "m", "file_name": "a.c", "code": "int x = 1;"}).to_string())
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
    assert_eq!(body["root"]["kind"], json!("unit"));
    assert!(
        body.get("spaces").is_none(),
        "the single root space must not be under the plural `spaces` key (#638)"
    );
}

#[actix_rt::test]
async fn test_web_v1_ping_works() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // The `/v1/ping` route works; the unprefixed `/ping` alias was removed
    // at 2.0 (#637) and now 404s (covered by `test_web_unprefixed_routes_now_404`).
    let resp =
        test::call_service(&app, test::TestRequest::get().uri("/v1/ping").to_request()).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET /v1/ping should return 200"
    );
}

#[actix_rt::test]
async fn test_web_post_to_ping_yields_405() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // `/v1/ping` is GET-only; its own `default_service` answers a method
    // error (not the 415 a content-type-guarded POST endpoint gives). The
    // unprefixed `/ping` alias is gone (#637), so a POST there is a 404.
    let resp =
        test::call_service(&app, test::TestRequest::post().uri("/v1/ping").to_request()).await;
    assert_eq!(
        resp.status(),
        StatusCode::METHOD_NOT_ALLOWED,
        "POST /v1/ping should return 405"
    );
    let body = test::read_body(resp).await;
    assert!(
        String::from_utf8_lossy(&body).contains("GET"),
        "405 body should name the accepted method"
    );
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
async fn test_web_v1_index_shape() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // `GET /v1` returns the machine-readable route index (#643): the issue's
    // pre-fix bug was that both `/v1` and `/` answered the app-level 404.
    let resp = test::call_service(&app, test::TestRequest::get().uri("/v1").to_request()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();

    assert_eq!(body["service"], json!("bca-web"));
    assert_eq!(body["version"], json!(env!("CARGO_PKG_VERSION")));
    let routes = body["routes"].as_array().expect("routes must be an array");
    // The index lists itself plus every other registered route.
    assert_eq!(
        routes.len(),
        ROUTES.len(),
        "index must serialize every ROUTES entry",
    );
    // Spot-check one entry's full shape so a silent field rename is caught.
    let self_entry = routes
        .iter()
        .find(|r| r["path"] == json!("/v1"))
        .expect("index must list itself");
    assert_eq!(self_entry["methods"], json!(["GET", "HEAD"]));
    assert!(
        self_entry["description"].is_string(),
        "each route needs a description",
    );
    // The vcs family — the documentation gap that motivated #643 — must be
    // discoverable from the index.
    for vcs_path in ["/v1/vcs", "/v1/vcs/trend", "/v1/vcs/jit"] {
        assert!(
            routes.iter().any(|r| r["path"] == json!(vcs_path)),
            "index must list {vcs_path}",
        );
    }
}

/// The route index must stay in lockstep with the actual routing table: a
/// new endpoint added to `register_endpoints` without a `ROUTES` entry (or
/// vice versa) would let the index silently drift from reality (#643). This
/// drives every advertised path through the live app and asserts it is
/// reachable (not the app-level 404), and that every method the index
/// advertises is honored.
#[actix_rt::test]
async fn test_web_v1_index_lists_every_registered_route() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    for entry in ROUTES {
        for method in entry.methods {
            let method = http::Method::from_bytes(method.as_bytes()).unwrap();
            let req = test::TestRequest::default()
                .method(method.clone())
                .uri(entry.path)
                .to_request();
            let resp = test::call_service(&app, req).await;
            // A registered route never produces the app-level 404; the POST
            // analysis routes answer 415 here (no Content-Type), but that is
            // the *resource's* fallback, proving the path is registered.
            assert_ne!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "advertised route {method} {} must be registered",
                entry.path,
            );
            // The index also promises which methods a route honors. Every
            // registered resource carries a 405 fallback for methods it does
            // NOT accept (`guarded_post_fallback` / `get_only_method_not_allowed`),
            // so an advertised method that the resource rejects answers 405.
            // Asserting `!= 405` here makes a wrong `methods` entry in `ROUTES`
            // fail (e.g. advertising GET for a POST-only route): without this
            // the `!= 404` check alone passed for any method, since the
            // fallback's 405/415 is also not a 404.
            assert_ne!(
                resp.status(),
                StatusCode::METHOD_NOT_ALLOWED,
                "advertised method {method} on {} must be honored, not 405",
                entry.path,
            );
        }
    }
}

#[actix_rt::test]
async fn test_web_v1_index_head_matches_get() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // HEAD must work on the index like every other GET route (#644).
    let get = test::call_service(&app, test::TestRequest::get().uri("/v1").to_request()).await;
    assert_eq!(get.status(), StatusCode::OK);
    let get_body = test::read_body(get).await;

    let head = test::call_service(
        &app,
        test::TestRequest::default()
            .method(http::Method::HEAD)
            .uri("/v1")
            .to_request(),
    )
    .await;
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(
        test::read_body(head).await,
        get_body,
        "HEAD /v1 must reach the GET handler",
    );
}

#[actix_rt::test]
async fn test_web_index_unprefixed_alias_removed_404s() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // The bare `/` index alias was removed at 2.0 (#637): it now 404s,
    // while `GET /v1` still serves the route index.
    let alias = test::call_service(&app, test::TestRequest::get().uri("/").to_request()).await;
    assert_eq!(
        alias.status(),
        StatusCode::NOT_FOUND,
        "the bare `/` index alias must 404 now it is removed (#637)",
    );
    assert!(
        !alias.headers().contains_key("deprecation"),
        "the removed `/` alias must not carry a Deprecation header",
    );

    let v1 = test::call_service(&app, test::TestRequest::get().uri("/v1").to_request()).await;
    assert_eq!(v1.status(), StatusCode::OK, "/v1 index must still work");
    let v1_body: Value = serde_json::from_slice(&test::read_body(v1).await).unwrap();
    assert_eq!(v1_body["service"], json!("bca-web"));
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

/// Asserts `body` parses as the uniform `{error, error_kind, id}` shape
/// with a non-empty `error` string, a non-empty `error_kind` machine
/// token (#631), and `id == expected_id`.
fn assert_uniform_error_body(body: &[u8], expected_id: &str) {
    let parsed: Value = serde_json::from_slice(body)
        .expect("error body must be valid JSON `{error, error_kind, id}`");
    assert!(
        parsed["error"].as_str().is_some_and(|s| !s.is_empty()),
        "error body must carry a non-empty `error` string: {parsed}"
    );
    assert!(
        parsed["error_kind"].as_str().is_some_and(|s| !s.is_empty()),
        "error body must carry a non-empty `error_kind` token (#631): {parsed}"
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

    // 1) JSON endpoint, invalid language -> 422 JSON `{error, id}` with
    //    the echoed id (issue #634: the route matched, the entity is
    //    unprocessable, so it is not a 404).
    let json_req = test::TestRequest::post()
        .uri("/v1/metrics")
        .insert_header(ContentType::json())
        .set_payload(
            json!({"id": "err-json", "file_name": "x.unknown_ext", "code": "x"}).to_string(),
        )
        .to_request();
    let json_resp = test::call_service(&app, json_req).await;
    assert_eq!(json_resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_uniform_error_body(&test::read_body(json_resp).await, "err-json");

    // 2) Octet-stream endpoint, invalid language -> 422 JSON `{error,
    //    id}` (formerly a bare `text/plain` "error: ..." body). The
    //    octet-stream variants carry no id, so `id` is the empty string.
    let octet_req = test::TestRequest::post()
        .uri("/v1/metrics?file_name=x.unknown_ext")
        .insert_header(ContentType::octet_stream())
        .set_payload("int x = 1;")
        .to_request();
    let octet_resp = test::call_service(&app, octet_req).await;
    assert_eq!(octet_resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
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

// --- #654: every analysis endpoint echoes the resolved language ---------

#[actix_rt::test]
async fn test_web_all_analysis_endpoints_echo_language() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // /ast, /comment, /function, and /metrics must all echo the resolved
    // language slug (#654 brings /ast into line with the other three). A
    // `.cpp` source resolves to the `cpp` slug everywhere.
    let cases = [
        (
            "/v1/ast",
            json!({"id": "p", "file_name": "a.cpp", "code": "int x = 1;", "comment": false, "span": false}),
        ),
        (
            "/v1/comment",
            json!({"id": "p", "file_name": "a.cpp", "code": "int x = 1; // c"}),
        ),
        (
            "/v1/function",
            json!({"id": "p", "file_name": "a.cpp", "code": "int f() { return 0; }"}),
        ),
        (
            "/v1/metrics",
            json!({"id": "p", "file_name": "a.cpp", "code": "int x = 1;"}),
        ),
    ];
    for (uri, body) in cases {
        let req = test::TestRequest::post()
            .uri(uri)
            .insert_header(ContentType::json())
            .set_payload(body.to_string())
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK, "{uri} should 200");
        let parsed: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
        assert_eq!(
            parsed["language"],
            json!("cpp"),
            "{uri} must echo the resolved language slug (#654)",
        );
    }
}

// --- #633: unknown payload fields are rejected, naming the offender -----

#[actix_rt::test]
async fn test_web_unknown_field_400s_naming_the_key() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // A typo'd field (`long_widnow`) on `/vcs` must 400 — not silently
    // drop the key and compute with defaults — naming the offender in the
    // human `error` and carrying the `unknown_field` machine token (#633 /
    // #631).
    let req = test::TestRequest::post()
        .uri("/v1/vcs")
        .insert_header(ContentType::json())
        .set_payload(
            json!({"id": "x", "repo_path": "/tmp/whatever", "long_widnow": "banana"}).to_string(),
        )
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "an unknown field must 400, not silently use defaults (#633)"
    );
    let body = test::read_body(resp).await;
    assert_uniform_error_body(&body, "");
    let parsed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        parsed["error_kind"],
        json!("unknown_field"),
        "an unknown field must carry the unknown_field token (#631/#633)"
    );
    assert!(
        parsed["error"]
            .as_str()
            .is_some_and(|e| e.contains("long_widnow")),
        "the 400 must name the offending key, got: {parsed}"
    );

    // The stale `unit` metrics flag is now an unknown field (#638 renamed
    // it to `scope`), so sending it 400s naming `unit` (migration note).
    let req = test::TestRequest::post()
        .uri("/v1/metrics")
        .insert_header(ContentType::json())
        .set_payload(
            json!({"id": "x", "file_name": "a.c", "code": "int x = 1;", "unit": false}).to_string(),
        )
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let parsed: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
    assert_eq!(parsed["error_kind"], json!("unknown_field"));
    assert!(
        parsed["error"].as_str().is_some_and(|e| e.contains("unit")),
        "the stale `unit` flag must be named as the offender, got: {parsed}"
    );
}

// --- `scope` query flag: `full` / `file` (#638) -------------------------

#[actix_rt::test]
async fn test_web_metrics_plain_scope_flag_accepts_full_and_file() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // `scope=file` (and case variants) returns the file-level root only:
    // the response's nested `spaces` list is empty. `scope=full`/absent
    // keep the full tree, which for this single-function source has a
    // non-empty nested `spaces` array. The root sits under `root` (#638).
    let post_scope = |scope: Option<&str>| {
        let uri = match scope {
            Some(v) => format!("/v1/metrics?file_name=u.py&scope={v}"),
            None => "/v1/metrics?file_name=u.py".to_string(),
        };
        test::TestRequest::post()
            .uri(&uri)
            .insert_header(ContentType::octet_stream())
            .set_payload("def foo():\n    pass\n")
            .to_request()
    };

    for file in ["file", "FILE", "File"] {
        let resp = test::call_service(&app, post_scope(Some(file))).await;
        assert_eq!(resp.status(), StatusCode::OK, "scope={file} should be 200");
        let body: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
        assert_eq!(
            body["root"]["spaces"],
            json!([]),
            "scope={file} should clear the nested children"
        );
    }

    for full in ["full", "FULL"] {
        let resp = test::call_service(&app, post_scope(Some(full))).await;
        assert_eq!(resp.status(), StatusCode::OK, "scope={full} should be 200");
        let body: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
        assert!(
            body["root"]["spaces"]
                .as_array()
                .is_some_and(|s| !s.is_empty()),
            "scope={full} should keep the nested children"
        );
    }

    // Absent defaults to `full` (full tree).
    let resp = test::call_service(&app, post_scope(None)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&test::read_body(resp).await).unwrap();
    assert!(
        body["root"]["spaces"]
            .as_array()
            .is_some_and(|s| !s.is_empty())
    );
}

#[actix_rt::test]
async fn test_web_metrics_plain_scope_flag_rejects_unknown() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    // The pre-2.0 boolean truthy/falsy set (`true`/`false`/`1`/`0`) is
    // gone (#638); only `full`/`file` are accepted. Any other value is a
    // 400 with the uniform JSON error body carrying the
    // `invalid_scope_flag` token.
    for bad in ["true", "false", "1", "0", "yes", "bogus"] {
        let req = test::TestRequest::post()
            .uri(&format!("/v1/metrics?file_name=u.py&scope={bad}"))
            .insert_header(ContentType::octet_stream())
            .set_payload("def foo():\n    pass\n")
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "scope={bad} should be rejected with 400"
        );
        let body = test::read_body(resp).await;
        assert_uniform_error_body(&body, "");
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            parsed["error_kind"],
            json!("invalid_scope_flag"),
            "scope={bad} should carry the invalid_scope_flag token"
        );
    }
}

// --- Introspection endpoints (#541; unprefixed aliases removed #637) -----

#[actix_rt::test]
async fn test_web_version_endpoint_reports_server_and_library() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;

    for uri in ["/v1/version"] {
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

    for uri in ["/v1/languages"] {
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

        // The opt-in Mozilla C++ dialect (#720) is name-selectable and
        // owns no extension, yet must still be listed for parity with the
        // Python `supported_languages()` surface. Under the pre-#720
        // `!extensions().is_empty()` filter it would be wrongly dropped,
        // so this also guards that the listing predicate is "not an
        // internal helper", not "has an extension".
        let mozcpp = languages
            .iter()
            .find(|entry| entry["name"] == json!("mozcpp"))
            .expect("`mozcpp` must be listed despite owning no extensions");
        assert_eq!(
            mozcpp["extensions"],
            json!([]),
            "mozcpp owns no file extensions"
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
    // The four constant stamps live once on the envelope (#635) and are
    // never repeated on a row.
    assert_eq!(res["recent_window_days"], 90);
    assert!(res["vcs_schema_version"].is_number());
    assert!(res["risk_score_version"].is_number());
    let files = res["files"].as_array().expect("files array");
    let work = files
        .iter()
        .find(|f| f["path"] == "work.rs")
        .expect("work.rs ranked");
    // The per-file metrics sit under a nested `vcs` key (#684), not
    // flattened beside `path`.
    let work_vcs = &work["vcs"];
    assert_eq!(work_vcs["commits_long"], 1);
    assert_eq!(work_vcs["commits_recent"], 1);
    for constant in [
        "vcs_schema_version",
        "risk_score_version",
        "long_window_days",
        "recent_window_days",
    ] {
        assert!(
            work.get(constant).is_none() && work_vcs.get(constant).is_none(),
            "constant `{constant}` must not be duplicated per row (#635)"
        );
    }
    // The bus-factor aggregate ships alongside the ranked files (#332):
    // one solo author over one file ⇒ repo bus factor 1.
    let bus_factor = &res["vcs_aggregate"]["bus_factor"];
    assert_eq!(bus_factor["repo"]["bus_factor"], 1);
    assert_eq!(bus_factor["repo"]["files"], 1);
    assert_eq!(bus_factor["bus_factor_schema_version"], 2);
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
async fn test_web_vcs_bad_window_is_400() {
    // An unparseable window is a client mistake, so `InvalidWindow` must
    // map to 400, not 500 — the same regression class #641 makes
    // structural via `vcs::Error::is_client_input`.
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
            "id": "req-win",
            "repo_path": repo.path().to_str().unwrap(),
            "long_window": "not-a-window",
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[actix_rt::test]
async fn test_web_vcs_bad_repo_path_is_consistently_400() {
    // Issue #653: a nonexistent `repo_path` (the most common client error —
    // a typo) and an existing-but-non-repo directory must BOTH answer a
    // consistent client-input 400. Before the fix the nonexistent path fell
    // through to a 500 (`gix::discover` maps a missing directory to the
    // environment-level `OpenRepository`), while the existing non-repo dir
    // already 400'd — so a typo'd path paged on-call for the client's own
    // mistake.
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .service(web::resource("/vcs").route(web::post().to(vcs_json))),
    )
    .await;
    // An existing directory that is definitely not a git repo. A fresh temp
    // dir is empty and outside any enclosing repository.
    let non_repo = tempfile::tempdir().expect("tempdir");
    let cases = [
        ("nonexistent", "/tmp/bca-does-not-exist-652653-xyzzy"),
        (
            "existing-non-repo",
            non_repo.path().to_str().expect("utf-8 temp path"),
        ),
    ];
    for (label, repo_path) in cases {
        let req = test::TestRequest::post()
            .uri("/vcs")
            .insert_header(ContentType::json())
            .set_json(json!({ "id": "rp", "repo_path": repo_path }))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "a {label} repo_path must be a 400, not a 500 (#653)"
        );
        let body: Value = test::read_body_json(resp).await;
        assert_eq!(body["id"], "rp");
        // Both bad-path shapes carry the same client-input token so a client
        // branches uniformly on "that path is not a repository".
        assert_eq!(
            body["error_kind"],
            json!("vcs_not_a_repository"),
            "a {label} repo_path must carry the not-a-repository token (#653)"
        );
    }
}

#[actix_rt::test]
async fn test_web_vcs_jit_commit_mode_nonexistent_repo_path_is_400() {
    // Issue #653 on the jit commit path: a nonexistent `repo_path` supplied
    // for commit scoring must 400, the same as `/vcs`, rather than 500.
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .service(web::resource("/vcs/jit").route(web::post().to(vcs_jit_json))),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/vcs/jit")
        .insert_header(ContentType::json())
        .set_json(json!({
            "id": "jit-rp",
            "repo_path": "/tmp/bca-does-not-exist-652653-jit",
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a nonexistent repo_path in jit commit mode must be a 400 (#653)"
    );
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body["error_kind"], json!("vcs_not_a_repository"));
}

#[actix_rt::test]
async fn vcs_error_response_maps_classification_to_status() {
    // The web boundary delegates to `vcs::Error::is_client_input` (#641):
    // a client-input variant is a 400 with the client-facing body, and an
    // environment/backend variant is a 500 with the generic failure body.
    // Asserting both branches directly pins the HTTP contract without
    // needing to provoke a backend failure through the endpoint.
    let client = vcs_error_response(
        &VcsError::InvalidWindow("banana".to_owned()),
        "id-1".to_owned(),
    );
    assert_eq!(client.status(), StatusCode::BAD_REQUEST);
    // The status and body are a triple: a 400 carries the *specific*
    // `vcs::Error` Display message (not the former kitchen-sink sentence),
    // its `error_kind` machine token, and the echoed id (#631).
    let client_body = actix_web::body::to_bytes(client.into_body()).await.unwrap();
    let client_json: Value = serde_json::from_slice(&client_body).unwrap();
    assert_eq!(
        client_json["error"],
        json!("invalid time window: banana"),
        "the 400 must carry the specific cause, not a kitchen-sink sentence",
    );
    assert_eq!(
        client_json["error_kind"],
        json!("vcs_invalid_window"),
        "the 400 must carry the per-variant machine token (#631)",
    );
    assert_eq!(client_json["id"], json!("id-1"));

    let internal = vcs_error_response(&VcsError::Walk("boom".to_owned()), "id-2".to_owned());
    assert_eq!(internal.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let internal_body = actix_web::body::to_bytes(internal.into_body())
        .await
        .unwrap();
    let internal_json: Value = serde_json::from_slice(&internal_body).unwrap();
    // The environment/backend 500 keeps the generic body and never leaks
    // the real cause, but still carries a machine token.
    assert_eq!(internal_json["error"], json!(VCS_FAILED));
    assert_eq!(internal_json["error_kind"], json!("vcs_internal_error"));
    assert_eq!(internal_json["id"], json!("id-2"));
}

#[actix_rt::test]
async fn vcs_error_kind_maps_each_client_variant_to_a_distinct_token() {
    // Locks the closed #631 token vocabulary: every `is_client_input`
    // variant gets its own distinct, non-internal token. `is_client_input`
    // (#641) owns 400-vs-500; `vcs_error_kind` owns which 400 token — this
    // pins all ten so a future variant or a renamed token cannot silently
    // collapse onto the generic internal token or collide with a sibling.
    use std::collections::HashSet;
    use std::path::PathBuf;

    let cases: [(VcsError, &str); 10] = [
        (
            VcsError::NotARepository(PathBuf::from("/x")),
            error_kind::VCS_NOT_A_REPOSITORY,
        ),
        (
            VcsError::ResolveRef {
                reference: "HEAD".to_owned(),
                reason: "gone".to_owned(),
            },
            error_kind::VCS_INVALID_REVISION,
        ),
        (
            VcsError::InvalidBotPattern("[".to_owned()),
            error_kind::VCS_INVALID_BOT_PATTERN,
        ),
        (
            VcsError::InvalidWindow("x".to_owned()),
            error_kind::VCS_INVALID_WINDOW,
        ),
        (
            VcsError::InvalidTimestamp("x".to_owned()),
            error_kind::VCS_INVALID_TIMESTAMP,
        ),
        (
            VcsError::InvalidFormula("x".to_owned()),
            error_kind::VCS_INVALID_FORMULA,
        ),
        (
            VcsError::InvalidFileTypeScope("x".to_owned()),
            error_kind::VCS_INVALID_FILE_TYPE_SCOPE,
        ),
        (
            VcsError::InvalidBusFactorThreshold("x".to_owned()),
            error_kind::VCS_INVALID_BUS_FACTOR_THRESHOLD,
        ),
        (
            VcsError::InvalidTrend("x".to_owned()),
            error_kind::VCS_INVALID_TREND,
        ),
        (
            VcsError::InvalidDiff("x".to_owned()),
            error_kind::VCS_INVALID_DIFF,
        ),
    ];
    let mut seen = HashSet::new();
    for (err, expected) in &cases {
        assert!(
            err.is_client_input(),
            "{err:?} must classify as client input"
        );
        let token = vcs_error_kind(err);
        assert_eq!(token, *expected, "wrong token for {err:?}");
        assert_ne!(
            token,
            error_kind::VCS_INTERNAL_ERROR,
            "client variant {err:?} must not collapse onto the internal token",
        );
        assert!(
            seen.insert(token),
            "token `{token}` is not distinct across client variants"
        );
    }
    // An environment/backend variant collapses onto the internal token.
    assert!(!VcsError::Walk("boom".to_owned()).is_client_input());
    assert_eq!(
        vcs_error_kind(&VcsError::Walk("boom".to_owned())),
        error_kind::VCS_INTERNAL_ERROR,
    );
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
        .uri("/v1/vcs")
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
    let req = test::TestRequest::get().uri("/v1/vcs").to_request();
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
        .uri("/v1/vcs/trend")
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
        .uri("/v1/vcs/trend")
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

/// #636: omitting `points` now succeeds with the CLI default of 12 (it
/// was formerly hard-required and 400'd when absent).
#[actix_rt::test]
async fn test_web_vcs_trend_points_defaults_to_twelve() {
    let repo = build_trend_repo();
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    // No `points` key — must default to 12, not 400.
    let payload = json!({
        "id": "trend-default",
        "repo_path": repo.path().to_str().unwrap(),
        "as_of": "@1700000000",
        "span": "300d",
    });
    let req = test::TestRequest::post()
        .uri("/v1/vcs/trend")
        .insert_header(ContentType::json())
        .set_json(&payload)
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "an omitted `points` must default to 12, not 400 (#636)"
    );
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(
        body["as_of_points"].as_array().unwrap().len(),
        12,
        "the default point count is the CLI's 12 (#636)"
    );
}

// --- POST /vcs/jit (issues #331 / #580) ----------------------------------

#[actix_rt::test]
async fn test_web_vcs_jit_commit_happy_path() {
    let repo = build_temp_repo();
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/v1/vcs/jit")
        .insert_header(ContentType::json())
        .set_json(json!({
            "id": "jit-1",
            "repo_path": repo.path().to_str().unwrap(),
            "commit": "HEAD",
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body["id"], "jit-1");
    assert_eq!(body["jit_schema_version"], 3);
    // Commit-mode reports carry the `source` discriminator (#642) the docs
    // promise, so clients branch on it rather than on key-absence.
    assert_eq!(body["source"], "commit");
    // A full commit report carries the score + every feature group.
    assert!(body["risk_score"].is_number());
    assert!(body["commit"]["id"].is_string());
    for group in ["size", "diffusion", "history", "experience"] {
        assert!(body["features"][group].is_object(), "features.{group}");
    }
}

#[actix_rt::test]
async fn test_web_vcs_jit_diff_mode_partial_report() {
    // A bare diff in the request body (issue #580): no repo needed. The
    // response is the partial report — `source: "diff"`, a `partial_risk_score`,
    // and NO history / experience / purpose / risk_score keys.
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let diff = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,1 +1,3 @@\n keep\n+one\n+two\n";
    let req = test::TestRequest::post()
        .uri("/v1/vcs/jit")
        .insert_header(ContentType::json())
        .set_json(json!({ "id": "jit-diff", "diff": diff }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body["id"], "jit-diff");
    assert_eq!(body["source"], "diff");
    assert!(body["partial_risk_score"].is_number());
    assert_eq!(body["size"]["lines_added"], 2);
    let obj = body.as_object().expect("object");
    // `partial_score` is the pre-#591 name for `partial_risk_score`; listing
    // it pins the rename so a serializer emitting both keys fails here.
    for absent in [
        "history",
        "experience",
        "purpose",
        "commit",
        "risk_score",
        "partial_score",
    ] {
        assert!(
            !obj.contains_key(absent),
            "diff report must omit `{absent}`"
        );
    }
}

#[actix_rt::test]
async fn test_web_vcs_jit_malformed_diff_is_400() {
    // The load-bearing regression (project memory / #515-style): a malformed
    // diff is a client mistake. The new `InvalidDiff` variant MUST map to
    // 400, not the catch-all 500, or it would fall through.
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/v1/vcs/jit")
        .insert_header(ContentType::json())
        .set_json(json!({
            "id": "jit-bad",
            "diff": "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ garbage @@\n",
        }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a malformed diff must be a 400, not a 500"
    );
    let body: Value = test::read_body_json(resp).await;
    // The body must name the problem (so a 400 is actionable, not opaque).
    assert_eq!(body["id"], "jit-bad");
    // Pin the machine token (#631): a malformed diff routes through the
    // specific `vcs_invalid_diff` cause, not the former kitchen-sink
    // bad-request string. The human `error` carries the specific
    // `vcs::Error::InvalidDiff` Display prefix.
    assert_eq!(
        body["error_kind"],
        json!("vcs_invalid_diff"),
        "the 400 must carry the per-variant machine token for a bad diff (#631)"
    );
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|e| e.starts_with("invalid unified diff:")),
        "the 400 must carry the specific diff-parse cause, got: {body}",
    );
}

#[actix_rt::test]
async fn test_web_vcs_jit_garbage_diff_is_400_not_zero_score() {
    // Issue #652: non-diff garbage in the `diff` field must be rejected with
    // a 400, NOT silently scored as a confident `partial_risk_score: 0.0`.
    // A 0.0 ("zero risk") on a risk-*gating* endpoint is the most dangerous
    // failure mode — a CI step feeding the wrong field would be told the
    // change is safe. The library `parse_unified_diff` accepts arbitrary
    // non-diff text as an empty diff, so the web layer must reject the
    // non-empty-but-zero-files case.
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/v1/vcs/jit")
        .insert_header(ContentType::json())
        .set_json(json!({ "id": "jit-garbage", "diff": "not a diff" }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "non-diff garbage must be a 400, not a 200 with a 0.0 score"
    );
    let body: Value = test::read_body_json(resp).await;
    assert_eq!(body["id"], "jit-garbage");
    // The 400 must name the cause as a bad diff (#631), the same token a
    // structurally-malformed diff carries, so a client branches on it
    // uniformly.
    assert_eq!(
        body["error_kind"],
        json!("vcs_invalid_diff"),
        "garbage diff must carry the bad-diff machine token (#652)"
    );
    // The score must NOT have leaked through: the response is an error body,
    // not a partial report.
    assert!(
        body.get("partial_risk_score").is_none(),
        "a rejected garbage diff must not carry a score: {body}"
    );
}

#[actix_rt::test]
async fn test_web_vcs_jit_empty_diff_is_valid_zero_score() {
    // Issue #652 boundary: an empty (or whitespace-only) diff legitimately
    // means "no changes" = zero risk, so it stays a valid 200 with a 0.0
    // score. Only non-empty, non-whitespace input that parses to zero files
    // is rejected. A CI step that computed an empty diff must still get the
    // zero-risk answer rather than a spurious 400.
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    for (label, diff) in [("empty", ""), ("whitespace", "   \n\t  \n")] {
        let req = test::TestRequest::post()
            .uri("/v1/vcs/jit")
            .insert_header(ContentType::json())
            .set_json(json!({ "id": "jit-empty", "diff": diff }))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "an {label} diff must stay a valid 200 (no changes = zero risk)"
        );
        let body: Value = test::read_body_json(resp).await;
        assert_eq!(body["source"], "diff");
        assert_eq!(
            body["size"]["files_touched"], 0,
            "an {label} diff touches no files"
        );
        assert_eq!(
            body["partial_risk_score"], 0.0,
            "an {label} diff scores a valid 0.0"
        );
    }
}

#[actix_rt::test]
async fn test_web_vcs_jit_missing_repo_path_is_400() {
    // Commit mode with no repo_path is a client mistake (no repository to
    // score), surfaced as the not-a-repository 400.
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/v1/vcs/jit")
        .insert_header(ContentType::json())
        .set_json(json!({ "id": "jit-norepo" }))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[actix_rt::test]
async fn test_web_vcs_jit_diff_with_commit_mode_field_is_400() {
    // Issue #632: a payload that combines `diff` with any commit-mode field
    // must be rejected with a 400 naming the conflict, not silently scored
    // as a diff (which would answer a different, non-comparable question).
    // Each case pairs `diff` with one conflicting field to prove every
    // commit-mode field trips the guard.
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let diff = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,1 +1,3 @@\n keep\n+one\n+two\n";
    let conflicting = [
        json!({ "id": "c", "diff": diff, "repo_path": "/tmp/whatever" }),
        json!({ "id": "c", "diff": diff, "commit": "HEAD" }),
        json!({ "id": "c", "diff": diff, "repo_path": "/tmp/whatever", "commit": "HEAD" }),
        json!({ "id": "c", "diff": diff, "long_window": "6mo" }),
        json!({ "id": "c", "diff": diff, "recent_window": "30d" }),
        json!({ "id": "c", "diff": diff, "full_history": true }),
        json!({ "id": "c", "diff": diff, "include_merges": true }),
        json!({ "id": "c", "diff": diff, "follow_renames": false }),
        json!({ "id": "c", "diff": diff, "as_of": "2024-01-01T00:00:00Z" }),
    ];
    for payload in conflicting {
        let req = test::TestRequest::post()
            .uri("/v1/vcs/jit")
            .insert_header(ContentType::json())
            .set_json(&payload)
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "combining diff with a commit-mode field must be a 400: {payload}"
        );
        let body: Value = test::read_body_json(resp).await;
        // The {error, id} body shape must hold: id echoed, error pins the
        // exact conflict message naming the fields (so the 400 is actionable
        // and not some unrelated 400 that merely mentions "diff").
        assert_eq!(body["id"], "c", "the correlation id must be echoed back");
        assert_eq!(
            body["error"].as_str(),
            Some(crate::web::vcs::VCS_JIT_MODE_CONFLICT),
            "the 400 body must pin the mode-conflict message: {payload}"
        );
    }
}

#[actix_rt::test]
async fn test_web_vcs_jit_wrong_method_yields_405() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let req = test::TestRequest::get().uri("/v1/vcs/jit").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::METHOD_NOT_ALLOWED,
        "GET /vcs/jit must be a 405, not a 404"
    );
    // The 405 and the 415 come from the same method-branching fallback, so
    // a swap of the two branches would still yield the right status. Assert
    // the body names the accepted method (POST) — mirroring the sibling
    // `/vcs` method test — so the branches can't be silently transposed.
    let body = test::read_body(resp).await;
    assert!(
        String::from_utf8_lossy(&body).contains("POST"),
        "405 body should name the accepted method"
    );
}

#[actix_rt::test]
async fn test_web_vcs_jit_wrong_content_type_yields_415() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/v1/vcs/jit")
        .insert_header((http::header::CONTENT_TYPE, "text/plain"))
        .set_payload("not json")
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(
        resp.status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "/vcs/jit wrong Content-Type must be a 415, not a 404"
    );
}

// --- Extractor / body-limit error paths must also return `{error, id}` ---
//
// Regression coverage for #639: actix extractor failures (malformed JSON,
// missing field, missing query param) and body-size rejections (413 on
// both the JSON and octet-stream paths) previously bypassed the published
// `{error, id}` JSON contract by emitting actix's default `text/plain` /
// HTML bodies. (The transport-read-error arm of `BodyError` shares the
// same `{error, id}` plumbing but has no in-harness reproduction — the
// test client can't sever a stream mid-body.) Each test below builds the
// app via
// the production `configure_routes`, so it exercises the real
// `JsonConfig` / `QueryConfig` error handlers, and asserts the response is
// `application/json` carrying a parseable `{error, id}` shape.

/// Asserts the response status is `expected`, its `Content-Type` is
/// `application/json`, and its body parses as `{error, id}` with a
/// non-empty `error` and the given `id`. The serde-derived `error`
/// message is returned for callers that want to assert on its content.
async fn assert_extractor_json_error(
    resp: actix_web::dev::ServiceResponse,
    expected: StatusCode,
    expected_id: &str,
) -> String {
    assert_eq!(resp.status(), expected);
    assert_eq!(
        resp.headers().get(http::header::CONTENT_TYPE).unwrap(),
        "application/json",
        "extractor error must be application/json, not actix's default plaintext"
    );
    let body = test::read_body(resp).await;
    assert_uniform_error_body(&body, expected_id);
    let parsed: Value = serde_json::from_slice(&body).unwrap();
    parsed["error"].as_str().unwrap().to_string()
}

#[actix_rt::test]
async fn test_web_malformed_json_body_yields_400_json() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/v1/ast")
        .insert_header(("content-type", "application/json"))
        // Syntactically invalid JSON: a bare opening brace.
        .set_payload("{")
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_extractor_json_error(resp, StatusCode::BAD_REQUEST, "").await;
}

#[actix_rt::test]
async fn test_web_missing_json_field_yields_400_json() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    // Valid JSON, but omits the required `file_name` field, so serde's
    // "missing field" error fires inside the extractor.
    let req = test::TestRequest::post()
        .uri("/v1/ast")
        .insert_header(("content-type", "application/json"))
        .set_payload(r#"{"id":"x","code":"int x;","comment":false,"span":true}"#)
        .to_request();

    let resp = test::call_service(&app, req).await;
    let msg = assert_extractor_json_error(resp, StatusCode::BAD_REQUEST, "").await;
    // serde's precise message is preserved as the `error` value.
    assert!(
        msg.contains("file_name"),
        "400 body should preserve serde's message naming the missing field: {msg}"
    );
}

#[actix_rt::test]
async fn test_web_missing_query_param_yields_400_json() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    // The octet-stream `/comment` endpoint requires the `file_name` query
    // param. Omitting it makes the `Query<WebCommentInfo>` extractor fail.
    let req = test::TestRequest::post()
        .uri("/v1/comment")
        .insert_header(ContentType::octet_stream())
        .set_payload("int x;")
        .to_request();

    let resp = test::call_service(&app, req).await;
    let msg = assert_extractor_json_error(resp, StatusCode::BAD_REQUEST, "").await;
    assert!(
        msg.contains("file_name"),
        "400 body should preserve serde's message naming the missing query param: {msg}"
    );
}

#[actix_rt::test]
async fn test_web_json_payload_too_large_yields_413_json() {
    // A small JSON limit with the production error handler attached, so the
    // 413 path is exercised without allocating megabytes.
    const TINY_LIMIT: usize = 64;
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .app_data(
                web::JsonConfig::default()
                    .limit(TINY_LIMIT)
                    .error_handler(json_error_handler),
            )
            .service(
                web::resource("/ast")
                    .guard(guard::Header("content-type", "application/json"))
                    .route(web::post().to(ast_parser)),
            ),
    )
    .await;
    let oversized = "a".repeat(TINY_LIMIT * 4);
    let req = test::TestRequest::post()
        .uri("/ast")
        .insert_header(("content-type", "application/json"))
        .set_payload(format!(
            r#"{{"id":"x","file_name":"foo.c","code":"{oversized}","comment":false,"span":true}}"#
        ))
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_extractor_json_error(resp, StatusCode::PAYLOAD_TOO_LARGE, "").await;
}

#[actix_rt::test]
async fn test_web_octet_payload_too_large_yields_413_json() {
    // The octet-stream 413 (enforced in `get_code`) must share the JSON
    // body shape with the JSON 413 above (#639 unifies the two formerly
    // divergent 413 bodies).
    const OCTET_LIMIT: usize = 16;
    let app = test::init_service(
        App::new()
            .app_data(test_config_with_body_limit(OCTET_LIMIT))
            .service(
                web::resource("/comment")
                    .guard(guard::Header("content-type", "application/octet-stream"))
                    .route(web::post().to(comment_removal_plain)),
            ),
    )
    .await;
    let oversized = "a".repeat(OCTET_LIMIT + 1);
    let req = test::TestRequest::post()
        .uri("/comment?file_name=foo.c")
        .insert_header(ContentType::octet_stream())
        .set_payload(oversized)
        .to_request();

    let resp = test::call_service(&app, req).await;
    let msg = assert_extractor_json_error(resp, StatusCode::PAYLOAD_TOO_LARGE, "").await;
    assert_eq!(msg, PAYLOAD_TOO_LARGE);
}

/// Posts `code` (Rust) to the JSON `/metrics` endpoint and returns the
/// parsed response body. Shared by the issue #640 EOL-parity tests below.
async fn metrics_for_rust_source(code: &str) -> Value {
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
            id: "640".to_string(),
            file_name: "test.rs".to_string(),
            code: code.to_string(),
            scope: Scope::Full,
        })
        .to_request();
    test::call_and_read_body_json(&app, req).await
}

#[actix_rt::test]
async fn test_web_metrics_unterminated_buffer_matches_cli() {
    // Issue #640: an editor buffer lacking a final newline used to report
    // `end_line: 0` and `sloc: 0` over the wire (the unit space ended before
    // its own child function) while the CLI reported `end_line: 1, sloc: 1`
    // on the same bytes via `read_file_with_eol`. The web handler now
    // normalises the buffer, so both surfaces agree.
    let res = metrics_for_rust_source("fn f(){}").await;
    let unit = &res["root"];
    assert_eq!(
        unit["end_line"], 1,
        "unit space must not end before its child"
    );
    assert_eq!(
        unit["metrics"]["loc"]["sloc"], 1.0,
        "sloc must be 1, not the pre-#640 0",
    );
}

#[actix_rt::test]
async fn test_web_metrics_eol_variants_match_terminated_lf() {
    // The canonical LF-terminated buffer is the reference; CRLF, lone-CR,
    // and missing-trailing-newline variants of the same content must all
    // normalise to the identical metric JSON (issue #640).
    let reference = metrics_for_rust_source("fn f(){}\n").await;
    for variant in ["fn f(){}", "fn f(){}\r\n", "fn f(){}\r"] {
        let got = metrics_for_rust_source(variant).await;
        assert_eq!(
            got, reference,
            "EOL variant {variant:?} must report the same metrics as the LF-terminated buffer",
        );
    }
}

#[actix_rt::test]
async fn test_web_comment_plain_preserves_crlf_unnormalised() {
    // The comment endpoints are deliberately exempt from #640 normalisation:
    // they return the comment-stripped source bytes to the caller, not a
    // derived metric. Applying `normalize_eol` here would append a trailing
    // `\n` the client never sent, mutating round-tripped content. This test
    // pins that the stripped output is NOT trailing-newline-padded — if the
    // comment path were folded into the normalisation, this assertion fails.
    let app = test::init_service(
        App::new().app_data(test_config()).service(
            web::resource("/comment")
                .guard(guard::Header("content-type", "application/octet-stream"))
                .route(web::post().to(comment_removal_plain)),
        ),
    )
    .await;
    // A real comment forces non-empty stripped output (the "no comments"
    // outcome is the empty body, #558); the surviving code carries no final
    // newline, which `normalize_eol` would otherwise add.
    let source = "int a = 1; // c\r\nint b = 2;";
    let req = test::TestRequest::post()
        .uri("/comment?file_name=foo.c")
        .insert_header(ContentType::octet_stream())
        .set_payload(source)
        .to_request();
    let body = test::call_and_read_body(&app, req).await;
    let body = String::from_utf8(body.to_vec()).expect("stripped output is valid UTF-8");
    assert!(
        !body.ends_with('\n'),
        "comment removal must not append a trailing newline, got {body:?}",
    );
}

// Issue #645: `id`, `comment`, and `span` are optional on the JSON
// request payloads. Omitting a defaulted field must succeed and produce
// the exact same response as sending its default explicitly — the
// previous behaviour was a needless `400 missing field`. Each test
// builds the app via the production `configure_routes` so it exercises
// the real `JsonConfig` extractor, then asserts the omit-field response
// equals the explicit-default response byte-for-byte.

/// POSTs `raw_json` to `uri` as `application/json` through the production
/// routing table and returns the parsed JSON response body.
async fn post_raw_json(uri: &str, raw_json: &str) -> Value {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let req = test::TestRequest::post()
        .uri(uri)
        .insert_header(("content-type", "application/json"))
        .set_payload(raw_json.to_string())
        .to_request();
    test::call_and_read_body_json(&app, req).await
}

#[actix_rt::test]
async fn test_web_ast_defaults_omitted_fields() {
    // Omitting `id`, `comment`, and `span` must match the explicit
    // `id: ""`, `comment: false`, `span: false` request (the `bool`
    // defaults and the empty-id "no correlation id" sentinel).
    let omitted = post_raw_json("/v1/ast", r#"{"file_name":"foo.c","code":"int x = 1;"}"#).await;
    let explicit = post_raw_json(
        "/v1/ast",
        r#"{"id":"","file_name":"foo.c","code":"int x = 1;","comment":false,"span":false}"#,
    )
    .await;
    assert_eq!(
        omitted, explicit,
        "omitting id/comment/span must equal the explicit-default request",
    );
    // The defaulted `id` echoes back empty, and `span: false` drops node
    // span objects — confirm the defaults actually took effect rather
    // than the two requests merely matching some other shape.
    assert_eq!(omitted["id"], json!(""));
    assert_eq!(omitted["root"]["type"], json!("translation_unit"));
    assert_eq!(
        omitted["root"]["span"],
        json!(null),
        "span: false must null out the span objects on the AST nodes",
    );
}

#[actix_rt::test]
async fn test_web_metrics_defaults_omitted_id() {
    let omitted = post_raw_json(
        "/v1/metrics",
        r#"{"file_name":"foo.c","code":"int x = 1;"}"#,
    )
    .await;
    let explicit = post_raw_json(
        "/v1/metrics",
        r#"{"id":"","file_name":"foo.c","code":"int x = 1;"}"#,
    )
    .await;
    assert_eq!(
        omitted, explicit,
        "omitting id must equal the explicit empty-id request",
    );
    assert_eq!(omitted["id"], json!(""));
}

#[actix_rt::test]
async fn test_web_comment_defaults_omitted_id() {
    let omitted = post_raw_json(
        "/v1/comment",
        r#"{"file_name":"foo.c","code":"int x = 1; // hi"}"#,
    )
    .await;
    let explicit = post_raw_json(
        "/v1/comment",
        r#"{"id":"","file_name":"foo.c","code":"int x = 1; // hi"}"#,
    )
    .await;
    assert_eq!(
        omitted, explicit,
        "omitting id must equal the explicit empty-id request",
    );
    assert_eq!(omitted["id"], json!(""));
}

#[actix_rt::test]
async fn test_web_function_defaults_omitted_id() {
    let omitted = post_raw_json(
        "/v1/function",
        r#"{"file_name":"foo.rs","code":"fn f(){}\n"}"#,
    )
    .await;
    let explicit = post_raw_json(
        "/v1/function",
        r#"{"id":"","file_name":"foo.rs","code":"fn f(){}\n"}"#,
    )
    .await;
    assert_eq!(
        omitted, explicit,
        "omitting id must equal the explicit empty-id request",
    );
    assert_eq!(omitted["id"], json!(""));
}

// ---------------------------------------------------------------------------
// Accept-header content negotiation (#657).
//
// These tests drive the production `configure_routes` router so they
// exercise the real handlers and the shared `negotiated_ok` path, asserting
// that each endpoint honours `Accept` for JSON / YAML / CBOR, defaults to
// JSON when `Accept` is absent or a wildcard, and answers `406` (through the
// uniform `{error, error_kind, id}` envelope) for an unsupported type.

/// A minimal Python metrics request body, reused across the negotiation
/// tests. Python is cheap to parse and produces a small space tree.
fn metrics_request_body() -> serde_json::Value {
    json!({
        "id": "neg-657",
        "file_name": "neg.py",
        "code": "def foo():\n    pass\n",
    })
}

/// Builds a `POST /v1/metrics` request, optionally setting `Accept`.
fn metrics_request(accept: Option<&str>) -> test::TestRequest {
    let mut builder = test::TestRequest::post()
        .uri("/v1/metrics")
        .insert_header(("content-type", "application/json"));
    if let Some(accept) = accept {
        builder = builder.insert_header(("accept", accept));
    }
    builder.set_payload(metrics_request_body().to_string())
}

/// The `Content-Type` of a finished response as a borrowed `&str`; an absent
/// or non-UTF-8 value yields the empty string so the assertion fails loudly.
fn content_type(resp: &actix_web::dev::ServiceResponse) -> &str {
    resp.headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
}

#[actix_rt::test]
async fn test_web_metrics_absent_accept_defaults_to_json() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let resp = test::call_service(&app, metrics_request(None).to_request()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(content_type(&resp), "application/json");
    let body = test::read_body(resp).await;
    let parsed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["id"], json!("neg-657"));
    assert_eq!(parsed["language"], json!("python"));
}

#[actix_rt::test]
async fn test_web_metrics_wildcard_accept_is_json() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let resp = test::call_service(&app, metrics_request(Some("*/*")).to_request()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(content_type(&resp), "application/json");
}

#[actix_rt::test]
async fn test_web_metrics_explicit_json_accept() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let resp =
        test::call_service(&app, metrics_request(Some("application/json")).to_request()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(content_type(&resp), "application/json");
}

#[actix_rt::test]
async fn test_web_metrics_yaml_accept_emits_yaml() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let resp =
        test::call_service(&app, metrics_request(Some("application/yaml")).to_request()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(content_type(&resp), "application/yaml");
    let body = test::read_body(resp).await;
    // The body must parse as YAML and carry the same fields the JSON body
    // would, proving the response is genuinely YAML and not JSON mislabelled.
    let parsed: serde_yaml::Value = serde_yaml::from_slice(&body).unwrap();
    assert_eq!(parsed["id"], serde_yaml::Value::from("neg-657"));
    assert_eq!(parsed["language"], serde_yaml::Value::from("python"));
    // YAML is a JSON superset, so a successful YAML parse alone does not rule
    // out a JSON body mislabelled as YAML. serde_yaml emits this nested map
    // in block style (`id: neg-657\n...`), which is not valid JSON — so a
    // failed JSON parse is positive proof the bytes are real YAML.
    assert!(
        serde_json::from_slice::<Value>(&body).is_err(),
        "YAML body must not also be valid JSON (would mean JSON mislabelled)",
    );
}

#[actix_rt::test]
async fn test_web_metrics_cbor_accept_emits_cbor() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let resp =
        test::call_service(&app, metrics_request(Some("application/cbor")).to_request()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(content_type(&resp), "application/cbor");
    let body = test::read_body(resp).await;
    // The body must decode as CBOR (binary, not text) and round-trip to a map.
    let parsed: ciborium::Value = ciborium::from_reader(body.as_ref()).unwrap();
    assert!(parsed.is_map(), "CBOR body must be a map, got {parsed:?}");
}

#[actix_rt::test]
async fn test_web_metrics_yaml_matches_json_payload() {
    // Cross-format parity: the YAML body and the JSON body must carry the
    // same data for the same input, differing only in serialization.
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let json_resp =
        test::call_service(&app, metrics_request(Some("application/json")).to_request()).await;
    let json_body = test::read_body(json_resp).await;
    let json: Value = serde_json::from_slice(&json_body).unwrap();

    let yaml_resp =
        test::call_service(&app, metrics_request(Some("application/yaml")).to_request()).await;
    let yaml_body = test::read_body(yaml_resp).await;
    let yaml: serde_yaml::Value = serde_yaml::from_slice(&yaml_body).unwrap();
    // Re-serialize the YAML value to JSON and compare structurally.
    let yaml_as_json: Value = serde_json::to_value(&yaml).unwrap();
    assert_eq!(json, yaml_as_json);
}

#[actix_rt::test]
async fn test_web_metrics_unsupported_accept_is_406() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let resp =
        test::call_service(&app, metrics_request(Some("application/xml")).to_request()).await;
    assert_eq!(resp.status(), StatusCode::NOT_ACCEPTABLE);
    // The 406 carries the uniform error envelope, not a bare body.
    assert_eq!(content_type(&resp), "application/json");
    let body = test::read_body(resp).await;
    let parsed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["error_kind"], json!("not_acceptable"));
    assert_eq!(parsed["id"], json!("neg-657"));
    // The human message lists the supported media types so a client can
    // self-correct without scraping the docs.
    let message = parsed["error"].as_str().unwrap();
    assert!(message.contains("application/yaml"), "got {message:?}");
    assert!(message.contains("application/cbor"), "got {message:?}");
}

#[actix_rt::test]
async fn test_web_metrics_q_weight_selects_yaml() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let resp = test::call_service(
        &app,
        metrics_request(Some("application/json;q=0.5, application/yaml;q=0.9")).to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(content_type(&resp), "application/yaml");
}

#[actix_rt::test]
async fn test_web_ast_yaml_accept_emits_yaml() {
    // Negotiation is shared across endpoints: prove a second endpoint honours
    // it too, guarding against a per-handler regression.
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/v1/ast")
        .insert_header(("content-type", "application/json"))
        .insert_header(("accept", "application/yaml"))
        .set_payload(ast_request_body().to_string())
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(content_type(&resp), "application/yaml");
    let body = test::read_body(resp).await;
    let parsed: serde_yaml::Value = serde_yaml::from_slice(&body).unwrap();
    assert_eq!(parsed["id"], serde_yaml::Value::from("ct-515"));
}

#[actix_rt::test]
async fn test_web_ast_unsupported_accept_is_406() {
    let app = test::init_service(
        App::new()
            .app_data(test_config())
            .configure(configure_routes),
    )
    .await;
    let req = test::TestRequest::post()
        .uri("/v1/ast")
        .insert_header(("content-type", "application/json"))
        .insert_header(("accept", "text/html"))
        .set_payload(ast_request_body().to_string())
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NOT_ACCEPTABLE);
    let body = test::read_body(resp).await;
    let parsed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["error_kind"], json!("not_acceptable"));
}

// --- CORS (`--cors`, #694) ---------------------------------------------
//
// CORS is opt-in and off by default. These tests build the app via the
// production `configure_routes` so they exercise the real routing, the
// per-resource OPTIONS/Allow fallback (#655), and the `cors_middleware`
// layered on top. `cors_app` mirrors `run_with_timeout`'s wiring: it
// registers the `CorsPolicy` as app data and wraps the `from_fn` middleware
// under a `Condition` keyed on whether the policy is enabled, so the default
// (`Disabled`) build carries no CORS layer at all.

use crate::web::cors::{CorsPolicy, cors_middleware};

/// Header name shorthands for the CORS assertions.
const ACAO: http::header::HeaderName = http::header::ACCESS_CONTROL_ALLOW_ORIGIN;
const ACAM: http::header::HeaderName = http::header::ACCESS_CONTROL_ALLOW_METHODS;
const ACAH: http::header::HeaderName = http::header::ACCESS_CONTROL_ALLOW_HEADERS;
const ACAC: http::header::HeaderName = http::header::ACCESS_CONTROL_ALLOW_CREDENTIALS;

/// Builds an app wired exactly like `run_with_timeout` for the given policy.
///
/// The `Condition` + `from_fn` wrapping yields an `EitherBody` response, so
/// the response body type is spelled out explicitly here.
fn cors_app(
    policy: CorsPolicy,
) -> App<
    impl actix_web::dev::ServiceFactory<
        actix_web::dev::ServiceRequest,
        Config = (),
        Response = actix_web::dev::ServiceResponse<
            actix_web::body::EitherBody<actix_web::body::BoxBody>,
        >,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    let enabled = policy != CorsPolicy::Disabled;
    App::new()
        .wrap(actix_web::middleware::Condition::new(
            enabled,
            actix_web::middleware::from_fn(cors_middleware),
        ))
        .app_data(test_config())
        .app_data(web::Data::new(policy))
        .configure(configure_routes)
}

#[actix_rt::test]
async fn test_cors_disabled_emits_no_headers() {
    let app = test::init_service(cors_app(CorsPolicy::Disabled)).await;
    // A cross-origin GET against an introspection route: with CORS off the
    // response must carry no `Access-Control-Allow-Origin` at all.
    let req = test::TestRequest::get()
        .uri("/v1/ping")
        .insert_header((http::header::ORIGIN, "https://app.example"))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers().get(ACAO).is_none(),
        "CORS off must emit no Access-Control-Allow-Origin"
    );
}

#[actix_rt::test]
async fn test_cors_disabled_preflight_has_no_cors_headers() {
    let app = test::init_service(cors_app(CorsPolicy::Disabled)).await;
    // The OPTIONS preflight still works as a method-discovery probe (204 +
    // Allow, #655), but carries no CORS decoration when CORS is off.
    let req = test::TestRequest::default()
        .method(http::Method::OPTIONS)
        .uri("/v1/metrics")
        .insert_header((http::header::ORIGIN, "https://app.example"))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(resp.headers().get(http::header::ALLOW).is_some());
    assert!(
        resp.headers().get(ACAO).is_none(),
        "preflight must not be CORS-decorated when CORS is off"
    );
}

#[actix_rt::test]
async fn test_cors_allow_list_echoes_listed_origin() {
    let app = test::init_service(cors_app(CorsPolicy::AllowList(vec![
        "https://app.example".to_owned(),
    ])))
    .await;
    let req = test::TestRequest::get()
        .uri("/v1/ping")
        .insert_header((http::header::ORIGIN, "https://app.example"))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    // A listed origin is echoed back verbatim (never `*`), with a
    // `Vary: Origin` so caches do not cross-pollinate origins.
    assert_eq!(resp.headers().get(ACAO).unwrap(), "https://app.example");
    assert_eq!(resp.headers().get(http::header::VARY).unwrap(), "Origin");
    // Credentials are never advertised (the API has no auth/cookies).
    assert!(resp.headers().get(ACAC).is_none());
}

#[actix_rt::test]
async fn test_cors_allow_list_blocks_unlisted_origin() {
    let app = test::init_service(cors_app(CorsPolicy::AllowList(vec![
        "https://app.example".to_owned(),
    ])))
    .await;
    let req = test::TestRequest::get()
        .uri("/v1/ping")
        .insert_header((http::header::ORIGIN, "https://evil.example"))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    // No header for an unlisted origin → the browser blocks the read.
    assert!(
        resp.headers().get(ACAO).is_none(),
        "an unlisted origin must receive no Access-Control-Allow-Origin"
    );
}

#[actix_rt::test]
async fn test_cors_wildcard_echoes_star() {
    let app = test::init_service(cors_app(CorsPolicy::Wildcard)).await;
    let req = test::TestRequest::get()
        .uri("/v1/ping")
        .insert_header((http::header::ORIGIN, "https://anything.example"))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    // The explicit wide-open opt-in answers every origin with a literal `*`,
    // and still never sets credentials (a `*` + credentials is invalid).
    assert_eq!(resp.headers().get(ACAO).unwrap(), "*");
    assert!(resp.headers().get(http::header::VARY).is_none());
    assert!(resp.headers().get(ACAC).is_none());
}

#[actix_rt::test]
async fn test_cors_preflight_advertises_methods_and_echoes_headers() {
    let app = test::init_service(cors_app(CorsPolicy::AllowList(vec![
        "https://app.example".to_owned(),
    ])))
    .await;
    // A real browser preflight: OPTIONS + Origin + the requested method and
    // headers. The middleware layers CORS on top of the #655 OPTIONS->204
    // handler, sourcing `Access-Control-Allow-Methods` from the resource's
    // own `Allow` header so it matches the real routing table.
    let req = test::TestRequest::default()
        .method(http::Method::OPTIONS)
        .uri("/v1/metrics")
        .insert_header((http::header::ORIGIN, "https://app.example"))
        .insert_header((http::header::ACCESS_CONTROL_REQUEST_METHOD, "POST"))
        .insert_header((
            http::header::ACCESS_CONTROL_REQUEST_HEADERS,
            "content-type, x-custom",
        ))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert_eq!(resp.headers().get(ACAO).unwrap(), "https://app.example");
    // `/metrics` is POST-only; the Allow header (POST, OPTIONS) is mirrored
    // into Access-Control-Allow-Methods verbatim.
    assert_eq!(resp.headers().get(ACAM).unwrap(), "POST, OPTIONS");
    // The requested headers are echoed back verbatim.
    assert_eq!(resp.headers().get(ACAH).unwrap(), "content-type, x-custom");
    assert!(resp.headers().get(ACAC).is_none());
}

#[actix_rt::test]
async fn test_cors_preflight_falls_back_to_default_headers() {
    let app = test::init_service(cors_app(CorsPolicy::Wildcard)).await;
    // A bare preflight with no Access-Control-Request-Headers gets the
    // documented static fallback set.
    let req = test::TestRequest::default()
        .method(http::Method::OPTIONS)
        .uri("/v1/metrics")
        .insert_header((http::header::ORIGIN, "https://app.example"))
        .to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert_eq!(resp.headers().get(ACAH).unwrap(), "Content-Type, Accept");
}

#[actix_rt::test]
async fn test_cors_allow_list_same_origin_request_gets_no_headers() {
    let app = test::init_service(cors_app(CorsPolicy::AllowList(vec![
        "https://app.example".to_owned(),
    ])))
    .await;
    // No Origin header → a same-origin request: the allow-list emits nothing
    // (and the `?` short-circuit must not panic).
    let req = test::TestRequest::get().uri("/v1/ping").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().get(ACAO).is_none());
}
