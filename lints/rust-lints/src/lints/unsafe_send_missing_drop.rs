use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::ty::implements_trait;
use rustc_hir::{Item, ItemKind, LangItem, Safety};
use rustc_lint::{LateContext, LateLintPass};
use rustc_middle::ty;
use rustc_span::sym;

rustc_session::declare_lint! {
    /// Warns when a type has `unsafe impl Send` but contains `!Send` fields
    /// and no `Drop` implementation, meaning the implicit destructor will drop
    /// those `!Send` fields on whatever thread happens to drop the owning struct.
    ///
    /// This is unsound when the `!Send` fields have thread-affinity requirements
    /// (e.g. ObjC pointers that must be released on a specific dispatch queue).
    pub UNSAFE_SEND_MISSING_DROP,
    Warn,
    "`unsafe impl Send` with `!Send` fields and no `Drop` impl"
}

pub struct UnsafeSendMissingDrop;

impl UnsafeSendMissingDrop {
    pub const fn new() -> Self {
        Self
    }
}

rustc_session::impl_lint_pass!(UnsafeSendMissingDrop => [UNSAFE_SEND_MISSING_DROP]);

impl<'tcx> LateLintPass<'tcx> for UnsafeSendMissingDrop {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        if item.span.from_expansion() {
            return;
        }
        let ItemKind::Impl(impl_block) = &item.kind else {
            return;
        };
        // Safety is on TraitImplHeader, not on Impl itself.
        let Some(trait_header) = impl_block.of_trait else {
            return;
        };
        if trait_header.safety != Safety::Unsafe {
            return;
        }
        let Some(trait_def_id) = trait_header.trait_ref.trait_def_id() else {
            return;
        };
        let Some(send_trait_id) = cx.tcx.get_diagnostic_item(sym::Send) else {
            return;
        };
        if trait_def_id != send_trait_id {
            return;
        }

        let self_ty = cx.tcx.type_of(item.owner_id.def_id).instantiate_identity();
        let ty::Adt(adt_def, args) = self_ty.kind() else {
            return;
        };

        // A Drop impl means the author has taken responsibility for destruction.
        if adt_def.destructor(cx.tcx).is_some() {
            return;
        }

        let has_non_send_field = adt_def.all_fields().any(|field| {
            let field_ty = field.ty(cx.tcx, args);
            let is_lang = |item| {
                field_ty
                    .ty_adt_def()
                    .is_some_and(|adt| cx.tcx.is_lang_item(adt.did(), item))
            };
            // ManuallyDrop<T> suppresses implicit destruction; PhantomData<T>
            // is a zero-sized marker with nothing to drop.
            if is_lang(LangItem::ManuallyDrop) || is_lang(LangItem::PhantomData) {
                return false;
            }
            // For unbounded generics (e.g. bare `T` with no `T: Send` bound),
            // the trait solver says "unknown" — treat as !Send because the
            // unsafe impl promises Send for ALL T.
            !implements_trait(cx, field_ty, send_trait_id, &[])
        });

        if !has_non_send_field {
            return;
        }

        let struct_span = cx.tcx.def_span(adt_def.did());
        let type_name = cx.tcx.item_name(adt_def.did());

        span_lint_and_help(
            cx,
            UNSAFE_SEND_MISSING_DROP,
            struct_span,
            format!(
                "`{type_name}` has `unsafe impl Send` but contains `!Send` fields and no `Drop` impl"
            ),
            None,
            "the implicit destructor drops `!Send` fields on the caller's thread; \
             implement `Drop` to ensure `!Send` fields are destroyed in the correct context",
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ui_unsafe_send_missing_drop() {
        crate::testing::run_ui_test("unsafe_send_missing_drop", None, &[]);
    }
}
