-- Installations de l'app (un jeton secret chacune, gardé haché) et profils publics de leurs
-- personnages.
-- Collation insensible à la casse : en _bin, MariaDB rend les CHAR comme du binaire, que sqlx ne lit pas en texte.
CREATE TABLE installs (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    token_hash CHAR(64) NOT NULL UNIQUE,
    created_at BIGINT NOT NULL,
    last_seen BIGINT NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE profiles (
    id CHAR(26) PRIMARY KEY,
    install_id BIGINT UNSIGNED NOT NULL,
    character_name VARCHAR(64) NOT NULL,
    data LONGTEXT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    UNIQUE KEY install_character (install_id, character_name),
    CONSTRAINT profiles_install FOREIGN KEY (install_id) REFERENCES installs (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
