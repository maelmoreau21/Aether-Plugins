use media_extension_sdk::{ExtensionRequest, ExtensionResponse};
use serde_json::json;
fn run(request: ExtensionRequest) -> ExtensionResponse {
    let title=request.input["title"].as_str().unwrap_or("Sans titre");
    let prefix=request.input["config"]["prefix"].as_str().unwrap_or("Mon fournisseur");
    let result=match request.operation.as_str() {
        "metadata.propose"=>json!({"title":title,"summary":format!("{prefix} : {title}")}),
        "metadata.search"=>json!([{ "id":"demo", "title":title, "mediaType":"movie" }]),
        "metadata.details"=>json!({"provider":"my-provider","providerId":"demo","title":title,"synopsis":format!("{prefix} : {title}")}),
        _=>json!({"error":"unsupported_operation"})
    };
    ExtensionResponse{api_version:1,result}
}
media_extension_sdk::extension!(run);
