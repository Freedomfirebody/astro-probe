#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fact {
    Class(ClassFact),
    Hierarchy(HierarchyFact),
    Method(MethodFact),
    Allocation(AllocationFact),
    Assignment(AssignmentFact),
    CallSite(CallSiteFact),
    CallArgument(CallArgumentFact),
    ClassAnnotation(ClassAnnotationFact),
    FieldAnnotation(FieldAnnotationFact),
    MethodAnnotation(MethodAnnotationFact),
    ParameterAnnotation(ParameterAnnotationFact),
    LibraryClass(LibraryClassFact),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassFact {
    pub fqn: String,
    pub kind: String, // "class" or "interface"
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HierarchyFact {
    pub class_fqn: String,
    pub parent_fqn: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodFact {
    pub method_fqn: String,
    pub class_fqn: String,
    pub method_name: String,
    pub params: String, // Comma-separated parameter names/types representation
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocationFact {
    pub alloc_id: String,
    pub class_fqn: String,
    pub method_fqn: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignmentFact {
    pub lhs: String,
    pub rhs: String,
    pub assignment_type: String, // "ALLOC", "COPY", "FIELD_READ", "FIELD_WRITE"
    pub method_fqn: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSiteFact {
    pub call_id: String,
    pub method_fqn: String,
    pub receiver: Option<String>,
    pub method_name: String,
    pub lhs: Option<String>,
    pub static_callee: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallArgumentFact {
    pub call_id: String,
    pub arg_index: usize,
    pub arg_var: String,
    pub arg_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassAnnotationFact {
    pub class_fqn: String,
    pub annotation_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldAnnotationFact {
    pub class_fqn: String,
    pub field_name: String,
    pub annotation_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodAnnotationFact {
    pub method_fqn: String,
    pub annotation_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterAnnotationFact {
    pub method_fqn: String,
    pub parameter_name: String,
    pub annotation_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryClassFact {
    pub fqn: String,
}
