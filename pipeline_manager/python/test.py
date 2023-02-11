from dbsp import DBSPConnection
from dbsp import ProjectConfig
from dbsp import InputEndpointConfig
from dbsp import OutputEndpointConfig
from dbsp import TransportConfig
from dbsp import FormatConfig
from dbsp import KafkaInputConfig

sql_code = """
CREATE TABLE demographics (
    cc_num FLOAT64 NOT NULL,
    first STRING,
    gender STRING,
    street STRING,
    city STRING,
    state STRING,
    zip INTEGER,
    lat FLOAT64,
    long FLOAT64,
    city_pop INTEGER,
    job STRING,
    dob STRING
);

CREATE TABLE transactions (
    trans_date_trans_time TIMESTAMP NOT NULL,
    cc_num FLOAT64 NOT NULL,
    merchant STRING,
    category STRING,
    amt FLOAT64,
    trans_num STRING,
    unix_time INTEGER,
    merch_lat FLOAT64,
    merch_long FLOAT64,
    is_fraud INTEGER
);

CREATE VIEW transactions_with_demographics as 
    SELECT
        transactions.trans_date_trans_time,
        transactions.cc_num,
        demographics.first,
        demographics.city
    FROM
        transactions JOIN demographics
        ON transactions.cc_num = demographics.cc_num;"""

def main():
    dbsp = DBSPConnection()
    print("Connection established")

    project = dbsp.new_project(name = "foo", sql_code = sql_code)
    print("Project created")

    status = project.status()
    print("Project status: " + status)
    
    config = ProjectConfig(project, 6)
    config.add_input(
            "DEMOGRAPHICS",
            InputEndpointConfig(
                transport = TransportConfig(
                    "kafka",
                    KafkaInputConfig(topics=['fraud_demo_large_demographics'], **{'bootstrap.servers': "localhost"})),
                format = FormatConfig("csv", )))

    project.compile()
    print("Project compiled")

    status = project.status()
    print("Project status: " + status)


if __name__ == "__main__":
    main()
