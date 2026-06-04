#[macro_export]
macro_rules! interactive_menu {
    (
        $menu_title:expr,
        $config_struct:expr,
        [
            $($field:ident: $label:expr => $kind:tt $([ $ty:path ])? $(display_as $disp_kind:tt)? $(if $cond:expr)?),* $(,)?
        ]
    ) => {{
        use console::style;
        use dialoguer::{Select, theme::ColorfulTheme};
        loop {
            let mut items = Vec::new();
            $(
                if true $(&& $cond)? {
                    let display_val = $crate::interactive_menu!( @pick_display $config_struct.$field, $kind $(, $disp_kind)? );
                    items.push(format!("{:<25} {}", $label, display_val));
                }
            )*
            items.push(style("Back").dim().to_string());

            println!("\n{}", style($menu_title).cyan().bold());
            println!("{}", style("━━━━━━━━━━━━━━━━━━━━━━━━━").dim());

            let sel = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Edit Setting")
                .items(&items)
                .default(items.len() - 1)
                .interact()?;

            if sel == items.len() - 1 {
                break;
            }

            let mut idx = 0;
            $(
                if true $(&& $cond)? {
                    if sel == idx {
                        $crate::interactive_menu!( @edit $config_struct, $field, $label, $kind $([ $ty ])? )?;
                    }
                    idx += 1;
                }
            )*
        }
        Ok::<(), anyhow::Error>(())
    }};

    ( @pick_display $val:expr, $kind:tt, $disp_kind:tt ) => {
        $crate::interactive_menu!( @display $val, $disp_kind )
    };
    ( @pick_display $val:expr, $kind:tt ) => {
        $crate::interactive_menu!( @display $val, $kind )
    };

    ( @display $val:expr, color) => { $crate::config::style_color($val) };
    ( @display $val:expr, num)   => { console::style($val.to_string()).yellow() };
    ( @display $val:expr, nonzero_num) => { $crate::interactive_menu!( @display $val, num) };
    ( @display $val:expr, bool)  => { console::style(if $val { "Yes" } else { "No" }).yellow() };
    ( @display $val:expr, text)  => { console::style(&$val).yellow() };
    ( @display $val:expr, enum)  => { console::style($val.label()).yellow() };
    ( @display $val:expr, opt_num) => {
        console::style($val.map(|v| v.to_string()).unwrap_or_else(|| "Auto".to_string())).yellow()
    };
    ( @display $val:expr, opt_text) => {
        console::style($val.as_deref().unwrap_or("(none)")).yellow()
    };
    ( @display $val:expr, kv) => {
        console::style(format!("{} items", $val.len())).yellow()
    };
    ( @display $val:expr, uploader) => {
        console::style($val.as_deref().unwrap_or("(none)")).yellow()
    };
    ( @display $val:expr, opt_enum) => {
        console::style($val.map(|v| v.label().to_string()).unwrap_or_else(|| "(none)".to_string())).yellow()
    };
    ( @display $val:expr, custom) => {
        console::style("(custom handler)".to_string()).yellow()
    };

    ( @edit $s:expr, $field:ident, $label:expr, text) => {{
        $s.$field = $crate::config::prompt_input($label, Some($s.$field.clone()))?;
        Ok::<(), anyhow::Error>(())
    }};
    ( @edit $s:expr, $field:ident, $label:expr, num) => {{
        $s.$field = $crate::config::prompt_input($label, Some($s.$field))?;
        Ok::<(), anyhow::Error>(())
    }};
    ( @edit $s:expr, $field:ident, $label:expr, nonzero_num) => {{
        $s.$field = $crate::config::prompt_input_validated($label, Some($s.$field), |v| {
            if *v == 0 {
                Err("Value must be greater than 0".to_string())
            } else {
                Ok(())
            }
        })?;
        Ok::<(), anyhow::Error>(())
    }};
    ( @edit $s:expr, $field:ident, $label:expr, bool) => {{
        $s.$field = $crate::config::prompt_confirm($label, $s.$field)?;
        Ok::<(), anyhow::Error>(())
    }};
    ( @edit $s:expr, $field:ident, $label:expr, color) => {{
        $s.$field = $crate::config::prompt_color($label, $s.$field)?;
        Ok::<(), anyhow::Error>(())
    }};
    ( @edit $s:expr, $field:ident, $label:expr, enum) => {{
        use $crate::config::types::ConfigEnum;
        let variants = $s.$field.variants_list();
        let current_idx = $s.$field.to_index();
        let sel = $crate::config::prompt_select($label, &variants, current_idx)?;
        if let Some(new_val) = $s.$field.at_index(sel) {
            $s.$field = new_val;
        }
        Ok::<(), anyhow::Error>(())
    }};
    ( @edit $s:expr, $field:ident, $label:expr, opt_num) => {{
        let current: u32 = $s.$field.map(|v| v.into()).unwrap_or(0);
        let val: u32 = $crate::config::prompt_input(&format!("{} (0 for Auto)", $label), Some(current))?;
        $s.$field = if val == 0 { None } else { Some(val.try_into().unwrap()) };
        Ok::<(), anyhow::Error>(())
    }};
    ( @edit $s:expr, $field:ident, $label:expr, opt_text) => {{
        $s.$field = $crate::config::prompt_optional_input($label, $s.$field.as_deref())?;
        Ok::<(), anyhow::Error>(())
    }};
    ( @edit $s:expr, $field:ident, $label:expr, kv) => {
        $crate::config::cli_ui::manage_kv_pairs($label, &mut $s.$field)
    };
    ( @edit $s:expr, $field:ident, $label:expr, uploader) => {{
        if $s.uploaders.is_empty() {
             $crate::config::cli_ui::print_error("No uploaders configured.");
        } else {
            let idx = $crate::config::cli_ui::select_uploader_index($s, $label)?;
            $s.$field = Some($s.uploaders[idx].name.clone());
        }
        Ok::<(), anyhow::Error>(())
    }};
    ( @edit $s:expr, $field:ident, $label:expr, opt_enum [ $enum_ty:path ]) => {{
        use $crate::config::types::ConfigEnum;
        let variants = <$enum_ty>::variants();
        let current_idx = $s.$field.map(|v| v.to_index()).unwrap_or(0);
        let sel = $crate::config::prompt_select($label, &variants, current_idx)?;
        $s.$field = <$enum_ty>::from_index(sel);
        Ok::<(), anyhow::Error>(())
    }};
    ( @edit $s:expr, $field:ident, $label:expr, custom [ $func:path ]) => {
        $func($s)
    };

    ( @display $val:expr, $unknown:tt ) => {{
        compile_error!(concat!("Unknown display kind passed to interactive_menu: ", stringify!($unknown)))
    }};
    ( @edit $s:expr, $field:ident, $label:expr, $unknown:tt $([ $ty:path ])? ) => {{
        compile_error!(concat!("Unknown edit kind passed to interactive_menu: ", stringify!($unknown)))
    }};
}
