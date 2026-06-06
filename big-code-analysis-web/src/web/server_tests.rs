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
    let body = test::read_body(resp).await;
    assert_eq!(String::from_utf8_lossy(&body), "Not found");
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
