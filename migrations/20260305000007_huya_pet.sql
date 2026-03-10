-- Huya: separate pet energy for /huyapet and per-day friend limit.

ALTER TABLE huya
    ADD COLUMN IF NOT EXISTS pet_energy_left     INT  NOT NULL DEFAULT 3,
    ADD COLUMN IF NOT EXISTS pet_energy_reset_at DATE NOT NULL DEFAULT CURRENT_DATE;

CREATE TABLE IF NOT EXISTS huya_pet_daily (
    chat_id     BIGINT NOT NULL,
    from_tg_id  BIGINT NOT NULL,
    target_tg_id BIGINT NOT NULL,
    day         DATE   NOT NULL,
    PRIMARY KEY (chat_id, from_tg_id, target_tg_id, day)
);

