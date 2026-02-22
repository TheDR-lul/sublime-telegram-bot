# Analysis: /menu — dimensions, UX, improvements

## Current implementation (summary)

- **Entry**: `/menu` deletes the command message, sends "Выберите раздел:" with inline keyboard.
- **Main keyboard**: Row1: "Игра Пидор дня" | "Прочее"; Row2: "Администрирование".
- **Submenus**: Game (Rules, Stats year, Stats all, Back); Other (About, Back); Admin (Pidor settings, Call unregistered, Back).
- **Auto-delete**: Menu message is scheduled for deletion 30 seconds after last interaction (same as rules).

## Limits and dimensions (Telegram Bot API)

| Constraint | Limit | Current /menu |
|------------|--------|----------------|
| `callback_data` length | 64 bytes (UTF-8) | All under (longest e.g. `menu:action:pidorcall` ~20 bytes) ✓ |
| Buttons per row | 14 max | Max 2 per row ✓ |
| Total reply_markup size | 4 kB | Small ✓ |
| Rows (practical) | ≤5 recommended on iOS | Max 4 rows in game submenu ✓ |

No changes required for API limits.

## Usability notes

1. **Layout**
   - Main: two columns for "Игра" and "Прочее" is good; "Администрирование" alone on second row is clear.
   - Submenus: every action is a single full-width button. On desktop, 4 columns are comfortable; current 1 column is safe but uses vertical space.

2. **Button labels**
   - Long labels: "Настройки автопидора", "Позвать незарегистрированных", "Статистика за всё время" — on narrow screens may wrap or feel heavy. Shortening can help (e.g. "Автопидор", "Позвать участников", "Статистика (всё)").
   - "← Назад" is consistent everywhere — good.

3. **Visual hierarchy**
   - RPG menu in the same project uses emoji (🧍 Profile, 🗺 World, 🎒 Inventory, 👥 Guild, ❌ Close). Main /menu has no emoji. Adding one emoji per section improves scanability and matches common bot patterns.

4. **Navigation**
   - Back always goes to main; submenus don’t cross-link. Logic is clear.
   - `answer_callback_query` is called for every callback — avoids client timeouts ✓.

## How known bots do it

- **@BotFather**: Uses command list (setMyCommands) for main entry; no custom inline menu. Settings are often step-by-step with short inline buttons.
- **@gamebot**: Inline keyboards under messages; 2–3 buttons per row; short labels; sometimes emoji for actions.
- **Common patterns**: 2–3 buttons per row; short, clear labels; one emoji per button for quick recognition; "Back" on its own row at the bottom; edit message on callback instead of sending new one (you already do this).

## Recommended improvements

1. **Add emoji to main and submenu buttons**  
   Same idea as RPG menu: one emoji per button (e.g. 🎮 Игра, 📋 Прочее, ⚙ Админ; in submenus: 📜 Правила, 📊 Статистика…). Improves scanability; no impact on `callback_data` (emoji only in button text).

2. **Compact game submenu**  
   Put the two stats in one row:  
   `[Статистика за год] [Статистика за всё время]`  
   Then: Rules (full width), Stats row, Back. Fewer rows, same actions.

3. **Shorten long labels (optional)**  
   If you want denser layout and less wrapping:  
   - "Настройки автопидора" → "Автопидор" or "Настройки автопидора" (tooltip/long form in description if needed).  
   - "Позвать незарегистрированных" → "Позвать участников".  
   - "Статистика за всё время" → "Статистика (всё)" or leave as is if space allows.

4. **Keep current behaviour**  
   - Edit message on callback (no extra messages). ✓  
   - Schedule delete after 30 s. ✓  
   - Admin checks on "menu:admin" and on actions. ✓  

## Optional (later)

- **callback_data**: If you ever need more than 64 bytes, use a short code (e.g. hash + nonce) and resolve server-side. Current payloads don’t require this.
- **Desktop**: You’re already within the recommended ≤4 columns; no change needed.
