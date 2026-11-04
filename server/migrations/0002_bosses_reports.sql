-- Boss des maps lus à l'écran (un par genre de map et par installation : le plus signalé fait
-- foi) et rapports d'erreurs de l'app.
CREATE TABLE map_bosses (
    install_id BIGINT UNSIGNED NOT NULL,
    code VARCHAR(80) NOT NULL,
    boss VARCHAR(120) NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (install_id, code),
    KEY code (code),
    CONSTRAINT map_bosses_install FOREIGN KEY (install_id) REFERENCES installs (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE reports (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    install_id BIGINT UNSIGNED NOT NULL,
    created_at BIGINT NOT NULL,
    app_version VARCHAR(32) NOT NULL,
    os VARCHAR(32) NOT NULL,
    character_name VARCHAR(64) NULL,
    entries LONGTEXT NOT NULL,
    log_tail LONGTEXT NOT NULL,
    KEY install_created (install_id, created_at),
    CONSTRAINT reports_install FOREIGN KEY (install_id) REFERENCES installs (id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
