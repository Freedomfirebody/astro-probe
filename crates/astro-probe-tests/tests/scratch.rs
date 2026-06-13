#[test]
fn test_debug_my_service_parsing() {
    let code = r#"
package com.test;
import org.springframework.beans.factory.annotation.Value;
import org.springframework.stereotype.Service;

@Service
public class MyService {
    @Value("${server.port:8080}")
    private String port;

    @Value("${app.name}")
    private String appName;

    @Value("${app.missing:default-val}")
    private String missing;

    @Value("literal-value")
    private String literal;

    public MyService(@Value("${app.desc}") String desc) {
    }
}
"#;
    let stripped = astro_probe_java::parser::strip_comments(code);
    println!("STRIPPED:\n{}", stripped);

    let (pkg, imports, _name, _kind, body, _parents) =
        astro_probe_java::parser::parse_package_and_imports(&stripped);

    // Call parse_class_body
    let (fields, _methods) =
        astro_probe_java::parser::parse_class_body(&pkg, &imports, "com.test.MyService", &body);

    println!("FIELDS: {:?}", fields);
    assert_eq!(fields.len(), 4);
}
