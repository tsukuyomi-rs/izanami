Usage

Generate the self-signed client certtificate:

```
$ ./private/gencert.sh
```

Test command:

```
curl --cacert ./private/server-crt.pem https://localhost:4000/
```
